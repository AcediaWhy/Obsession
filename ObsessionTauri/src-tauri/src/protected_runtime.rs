//! Medium-integrity bridge to the protected Windows runtime service.
//!
//! Only typed, bounded protocol values cross this boundary. In particular,
//! the UI cannot provide an executable path, a config path or raw arguments.

pub(crate) mod legacy_test;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, DpiLaunchSpec, DpiRuntimeSnapshot};
use crate::util::{self, DpiStatusPayload, LockExt, VersionedSection};
use obsession_runtime_protocol::{
    DpiEngine, HostsCheckRequest, HostsHealthSnapshot, HostsMutationRequest, Request,
};

#[cfg(windows)]
use obsession_runtime_client::RuntimeClient;
#[cfg(windows)]
use obsession_runtime_protocol::{
    Capabilities, DpiCategory, DpiReplaceRequest, DpiRuntimeOptions, DpiSelection, DpiStartRequest,
    DpiStopRequest, Feature, LegacyAssessmentClassification as ProtocolClassification,
    LegacyAssessmentConfidence as ProtocolConfidence, LegacyLanePhase as ProtocolLanePhase,
    LegacyObserverHealth, LegacyRecoveryApprovalRequest, LegacyRecoveryAttemptPhase,
    LegacyRecoveryCompletionDisposition, LegacyRecoveryControlsRequest,
    LegacyRecoveryMode as ProtocolRecoveryMode, LegacyRecoveryOrigin as ProtocolRecoveryOrigin,
    Response, RuntimeSnapshot, ServiceErrorCode, Zapret2AdaptiveFunction, Zapret2AdaptiveOverride,
    Zapret2AdaptivePayload, Zapret2AdaptiveRange, Zapret2AdaptiveStep, Zapret2AdaptiveTransport,
    Zapret2AdaptiveValue, ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
};

const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(750);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const HOSTS_OPERATION_TIMEOUT: Duration = Duration::from_secs(90);
const PROXY_LAN_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LEGACY_MONITOR_EPOCH: AtomicU64 = AtomicU64::new(0);
static DPI_PROJECTION: OnceLock<Mutex<DpiProjection>> = OnceLock::new();

const LEGACY_MONITOR_INTERVAL: Duration = Duration::from_secs(1);
const LEGACY_MONITOR_FAILURES_BEFORE_INACTIVE: u8 = 4;

/// Additive, UI-safe projection of the authenticated service contract.
/// Availability means the capabilities request completed successfully; each
/// privileged feature remains independently fail-closed.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedRuntimeCapabilities {
    pub service_available: bool,
    pub service_version: Option<String>,
    pub dpi: bool,
    pub zapret2: bool,
    pub adaptive_zapret2: bool,
    pub eyes_events: bool,
    pub legacy_reliability_controls: bool,
    pub legacy_reliability: bool,
    pub hosts: bool,
    pub proxy_lan_firewall: bool,
}

#[cfg(windows)]
fn project_capabilities(capabilities: Capabilities) -> ProtectedRuntimeCapabilities {
    let has = |feature| capabilities.features.contains(&feature);
    ProtectedRuntimeCapabilities {
        service_available: true,
        service_version: Some(capabilities.service_version),
        dpi: has(Feature::Dpi),
        zapret2: has(Feature::DpiZapret2),
        adaptive_zapret2: has(Feature::DpiZapret2) && has(Feature::DpiZapret2Adaptive),
        eyes_events: has(Feature::EyesEvents),
        legacy_reliability_controls: has(Feature::LegacyReliabilityControls),
        legacy_reliability: has(Feature::Dpi)
            && has(Feature::EyesEvents)
            && has(Feature::LegacyReliability),
        hosts: has(Feature::Hosts),
        proxy_lan_firewall: has(Feature::ProxyLanFirewall),
    }
}

#[derive(Default)]
struct DpiProjection {
    generation: Option<u64>,
    engine: Option<DpiEngine>,
    selections: Vec<(String, String)>,
    zapret2_level: Option<u8>,
    adaptive_overrides: BTreeMap<String, crate::adaptive_strategy::dsl::StrategyCandidate>,
    started_at_unix: Option<u64>,
}

fn projection() -> &'static Mutex<DpiProjection> {
    DPI_PROJECTION.get_or_init(|| Mutex::new(DpiProjection::default()))
}

fn lock_projection() -> std::sync::MutexGuard<'static, DpiProjection> {
    projection()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn next_request_id(operation: &str) -> String {
    let sequence = REQUEST_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    format!("ui-{operation}-{}-{sequence}", std::process::id())
}

#[cfg(windows)]
fn service_error_message(operation: &str, code: ServiceErrorCode) -> &'static str {
    let hosts_operation = operation.starts_with("hosts-");
    let proxy_operation = operation.starts_with("proxy-lan-");
    match (hosts_operation, proxy_operation, code) {
        (_, _, ServiceErrorCode::AccessDenied) => "служба отклонила удостоверение приложения",
        (true, _, ServiceErrorCode::Busy) => "защищённая операция с hosts уже выполняется",
        (true, _, ServiceErrorCode::Conflict) => {
            "файл hosts изменился прямо во время операции; повторите попытку"
        }
        (true, _, ServiceErrorCode::InvalidRequest) => "служба отклонила параметры hosts",
        (true, _, ServiceErrorCode::ProtectedResourceInvalid) => {
            "защищённые данные hosts отсутствуют или повреждены"
        }
        (true, _, ServiceErrorCode::RuntimeFailed) => "служба не смогла применить hosts",
        (true, _, ServiceErrorCode::ServiceUnavailable) => {
            "управление hosts недоступно в этой службе"
        }
        (_, true, ServiceErrorCode::Busy) => "настройка доступа к proxy уже выполняется",
        (_, true, ServiceErrorCode::Conflict) => {
            "состояние доступа к proxy изменилось; повторите операцию"
        }
        (_, true, ServiceErrorCode::InvalidRequest) => "служба отклонила параметры доступа к proxy",
        (_, true, ServiceErrorCode::RuntimeFailed) => "служба не смогла изменить доступ к proxy",
        (_, true, ServiceErrorCode::ServiceUnavailable) => {
            "управление доступом к proxy недоступно в этой службе"
        }
        (_, _, ServiceErrorCode::Busy) => "защищённый DPI runtime занят другой операцией",
        (_, _, ServiceErrorCode::Conflict) => "состояние DPI изменилось; повторите операцию",
        (_, _, ServiceErrorCode::IncompatibleProtocol) => "версия защищённой службы несовместима",
        (_, _, ServiceErrorCode::InvalidRequest) => "служба отклонила параметры DPI",
        (_, _, ServiceErrorCode::ProtectedResourceInvalid) => {
            "защищённые файлы DPI отсутствуют или не прошли проверку целостности"
        }
        (_, _, ServiceErrorCode::RuntimeFailed) => "защищённый DPI runtime не смог запуститься",
        (_, _, ServiceErrorCode::ServiceUnavailable) => "возможность DPI недоступна в этой службе",
        (_, _, ServiceErrorCode::Internal) => "внутренняя ошибка защищённой службы",
    }
}

#[cfg(windows)]
fn call_service(operation: &str, request: Request, timeout: Duration) -> Result<Response, String> {
    let client = RuntimeClient::new(timeout)
        .map_err(|error| format!("не удалось создать runtime-client: {error}"))?;
    let response = client
        .call(next_request_id(operation), request)
        .map_err(|error| format!("защищённая служба Obsession недоступна: {error}"))?;
    match response.response {
        Response::Error(error) => Err(format!(
            "защищённая служба отклонила операцию: {}",
            service_error_message(operation, error.code)
        )),
        response => Ok(response),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct ReplaceCallError {
    code: Option<ServiceErrorCode>,
    message: String,
}

#[cfg(windows)]
fn call_service_for_replace(
    operation: &str,
    request: Request,
    timeout: Duration,
) -> Result<Response, ReplaceCallError> {
    let client = RuntimeClient::new(timeout).map_err(|error| ReplaceCallError {
        code: None,
        message: format!("не удалось создать runtime-client: {error}"),
    })?;
    let response = client
        .call(next_request_id(operation), request)
        .map_err(|error| ReplaceCallError {
            code: None,
            message: format!("защищённая служба Obsession недоступна: {error}"),
        })?;
    match response.response {
        Response::Error(error) => Err(ReplaceCallError {
            code: Some(error.code),
            message: format!(
                "защищённая служба отклонила операцию: {}",
                service_error_message(operation, error.code)
            ),
        }),
        response => Ok(response),
    }
}

#[cfg(not(windows))]
fn unavailable() -> String {
    "защищённая служба Obsession поддерживается только в Windows".to_string()
}

#[cfg(windows)]
fn capabilities() -> Result<Capabilities, String> {
    match call_service("capabilities", Request::GetCapabilities, DISCOVERY_TIMEOUT)? {
        Response::Capabilities(capabilities) => Ok(capabilities),
        _ => Err("защищённая служба вернула неожиданный ответ capabilities".to_string()),
    }
}

pub fn capability_snapshot() -> ProtectedRuntimeCapabilities {
    #[cfg(windows)]
    {
        capabilities().map(project_capabilities).unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        ProtectedRuntimeCapabilities::default()
    }
}

pub fn hosts_runtime_snapshot(
) -> Result<Option<obsession_runtime_protocol::HostsRuntimeSnapshot>, String> {
    #[cfg(windows)]
    {
        runtime_snapshot_blocking(DISCOVERY_TIMEOUT).map(|snapshot| snapshot.hosts)
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}

pub async fn hosts_install(
    provider: obsession_runtime_protocol::HostsProvider,
) -> Result<(), String> {
    hosts_mutation(
        "hosts-install",
        Request::HostsInstall(HostsMutationRequest { provider }),
    )
    .await
}

pub async fn hosts_check(max_age_seconds: u32) -> Result<HostsHealthSnapshot, String> {
    #[cfg(windows)]
    {
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service(
                "hosts-check",
                Request::HostsCheck(HostsCheckRequest { max_age_seconds }),
                HOSTS_OPERATION_TIMEOUT,
            )
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        match response {
            Response::HostsHealth(snapshot) => Ok(snapshot),
            _ => Err("защищённая служба вернула неожиданный ответ hosts check".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = max_age_seconds;
        Err(unavailable())
    }
}

pub async fn hosts_uninstall() -> Result<(), String> {
    hosts_mutation("hosts-uninstall", Request::HostsUninstall).await
}

pub async fn hosts_restore(
    provider: obsession_runtime_protocol::HostsProvider,
) -> Result<(), String> {
    hosts_mutation(
        "hosts-restore",
        Request::HostsRestoreLastKnownGood(HostsMutationRequest { provider }),
    )
    .await
}

pub async fn proxy_lan_firewall_open(port: u16, lease_seconds: u16) -> Result<(), String> {
    #[cfg(windows)]
    {
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service(
                "proxy-lan-open",
                Request::FirewallOpenProxyLan(
                    obsession_runtime_protocol::FirewallOpenProxyLanRequest {
                        port,
                        lease_seconds,
                    },
                ),
                PROXY_LAN_OPERATION_TIMEOUT,
            )
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        match response {
            Response::Accepted(_) => Ok(()),
            _ => Err("защищённая служба вернула неожиданный ответ firewall open".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (port, lease_seconds);
        Err(unavailable())
    }
}

pub async fn proxy_lan_firewall_close() -> Result<(), String> {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(proxy_lan_firewall_close_blocking)
            .await
            .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))?
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}

pub(crate) fn proxy_lan_firewall_close_blocking() -> Result<(), String> {
    #[cfg(windows)]
    {
        match call_service(
            "proxy-lan-close",
            Request::FirewallCloseProxyLan,
            PROXY_LAN_OPERATION_TIMEOUT,
        )? {
            Response::Stopped => Ok(()),
            _ => Err("защищённая служба вернула неожиданный ответ firewall close".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}

async fn hosts_mutation(operation: &'static str, request: Request) -> Result<(), String> {
    #[cfg(windows)]
    {
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service(operation, request, HOSTS_OPERATION_TIMEOUT)
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        match response {
            Response::Accepted(_) => Ok(()),
            _ => Err("защищённая служба вернула неожиданный ответ hosts".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (operation, request);
        Err(unavailable())
    }
}

pub fn dpi_available() -> bool {
    #[cfg(windows)]
    {
        capabilities()
            .map(|capabilities| capabilities.features.contains(&Feature::Dpi))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Zapret2 stays fail-closed until the authenticated service advertises its
/// typed server-side Strategy Pack compiler explicitly.
pub fn zapret2_available() -> bool {
    #[cfg(windows)]
    {
        capabilities()
            .map(|capabilities| capabilities.features.contains(&Feature::DpiZapret2))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn adaptive_zapret2_available() -> bool {
    #[cfg(windows)]
    {
        capabilities()
            .map(|capabilities| {
                capabilities.features.contains(&Feature::DpiZapret2)
                    && capabilities.features.contains(&Feature::DpiZapret2Adaptive)
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Recovery requires more than packet observations: the service must own the
/// exact generation-fenced mutation/rollback protocol as well.
pub fn legacy_reliability_available() -> bool {
    #[cfg(windows)]
    {
        capabilities()
            .map(|capabilities| {
                capabilities.features.contains(&Feature::Dpi)
                    && capabilities.features.contains(&Feature::EyesEvents)
                    && capabilities.features.contains(&Feature::LegacyReliability)
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn runtime_snapshot_blocking(timeout: Duration) -> Result<RuntimeSnapshot, String> {
    match call_service("snapshot", Request::GetRuntimeSnapshot, timeout)? {
        Response::RuntimeSnapshot(snapshot) => Ok(snapshot),
        _ => Err("защищённая служба вернула неожиданный runtime snapshot".to_string()),
    }
}

#[cfg(windows)]
fn applied_legacy_recovery_config(snapshot: &RuntimeSnapshot) -> Option<(u64, &'static str, &str)> {
    let runtime = snapshot.dpi.as_ref()?;
    let reliability = snapshot.legacy_reliability.as_ref()?;
    let completion = reliability.recovery.last_completion.as_ref()?;
    if runtime.engine != DpiEngine::Legacy
        || reliability.generation != runtime.generation
        || completion.phase != LegacyRecoveryAttemptPhase::Applied
        || completion.disposition != LegacyRecoveryCompletionDisposition::CandidateApplied
        || !reliability.active_categories.contains(&completion.category)
        || !runtime.selections.iter().any(|selection| {
            selection.category == completion.category
                && selection.strategy_id == completion.candidate_config_id
        })
    {
        return None;
    }
    Some((
        completion.attempt_id,
        protocol_category_name(completion.category),
        completion.candidate_config_id.as_str(),
    ))
}

#[cfg(windows)]
fn reconcile_legacy_recovery_snapshot(
    app: &AppHandle,
    snapshot: &RuntimeSnapshot,
) -> Result<Option<u64>, String> {
    let Some((attempt_id, category, candidate_config)) = applied_legacy_recovery_config(snapshot)
    else {
        return Ok(None);
    };
    crate::commands::persist_recovered_legacy_config(app, category, candidate_config)?;
    Ok(Some(attempt_id))
}

pub(crate) fn reconcile_legacy_recovery_selection(app: &AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        let snapshot = runtime_snapshot_blocking(DISCOVERY_TIMEOUT)?;
        reconcile_legacy_recovery_snapshot(app, &snapshot)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

#[cfg(windows)]
fn legacy_controls_request(settings: &crate::settings::Settings) -> Result<Request, String> {
    let mode = if !settings.legacy_reliability_enabled {
        ProtocolRecoveryMode::ObserveOnly
    } else {
        match settings.legacy_reliability_mode.as_str() {
            "observe_only" => ProtocolRecoveryMode::ObserveOnly,
            "assisted" => ProtocolRecoveryMode::Assisted,
            "automatic" => ProtocolRecoveryMode::Automatic,
            value => return Err(format!("неизвестный режим Legacy recovery: {value}")),
        }
    };
    let frozen_categories = settings
        .legacy_reliability_frozen_categories
        .iter()
        .map(|value| category(value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Request::SetLegacyRecoveryControls(
        LegacyRecoveryControlsRequest {
            mode,
            automatic_paused: mode != ProtocolRecoveryMode::Automatic
                || settings.legacy_automatic_paused,
            frozen_categories,
        },
    ))
}

/// Best-effort discovery, strict write: an older/missing service leaves the
/// desired user settings pending for the next bootstrap, while a service that
/// explicitly advertises controls must acknowledge the typed update.
pub fn sync_legacy_recovery_controls(
    settings: &crate::settings::Settings,
) -> Result<Option<u64>, String> {
    #[cfg(windows)]
    {
        let Ok(capabilities) = capabilities() else {
            return Ok(None);
        };
        if !capabilities
            .features
            .contains(&Feature::LegacyReliabilityControls)
        {
            return Ok(None);
        }
        match call_service(
            "legacy-controls",
            legacy_controls_request(settings)?,
            OPERATION_TIMEOUT,
        )? {
            Response::Accepted(accepted) => Ok(Some(accepted.operation_id)),
            _ => Err("защищённая служба вернула неожиданный ответ Legacy controls".to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = settings;
        Ok(None)
    }
}

pub async fn approve_legacy_recovery(
    app: &AppHandle,
    approval: crate::legacy_reliability::recovery::AssistedApproval,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let request = Request::ApproveLegacyRecovery(LegacyRecoveryApprovalRequest {
            proposal_id: approval.proposal_id().get(),
            attempt_id: approval.attempt_id().get(),
        });
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service("legacy-approve", request, OPERATION_TIMEOUT)
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        match response {
            Response::Accepted(_) => {
                // Approval can synchronously replace the protected runtime
                // generation before the next poll. The monitor follows the
                // service-owned observer, not the generation that happened to
                // exist when the user clicked Approve.
                spawn_legacy_reliability_monitor(app, None);
                Ok(())
            }
            _ => Err("защищённая служба вернула неожиданный ответ Legacy approval".into()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, approval);
        Err(unavailable())
    }
}

fn inactive_status() -> DpiStatusPayload {
    DpiStatusPayload {
        active: false,
        processes: Vec::new(),
        started_at: None,
    }
}

#[cfg(windows)]
fn project_snapshot(snapshot: RuntimeSnapshot) -> DpiStatusPayload {
    let Some(runtime) = snapshot.dpi else {
        *lock_projection() = DpiProjection::default();
        return inactive_status();
    };

    let mut projection = lock_projection();
    if projection.generation != Some(runtime.generation) {
        projection.generation = Some(runtime.generation);
        projection.engine = Some(runtime.engine);
        projection.selections = runtime
            .selections
            .iter()
            .map(|selection| {
                (
                    category_name(selection.category).to_string(),
                    selection.strategy_id.clone(),
                )
            })
            .collect();
        // The public service snapshot intentionally omits adaptive DSL and
        // aggression level. A UI attaching to somebody else's generation may
        // observe/stop it, but cannot replace it without an exact local spec.
        projection.zapret2_level = None;
        projection.adaptive_overrides.clear();
        // The service protocol currently exposes no wall-clock start time.
        // When attaching to a pre-existing generation, start the UI uptime at
        // the moment this authenticated snapshot was first observed.
        projection.started_at_unix = Some(util::unix_secs());
    }
    DpiStatusPayload {
        active: true,
        // Process identities intentionally stay service-owned. An empty list
        // means "active through the system service", not an inactive runtime.
        processes: Vec::new(),
        started_at: projection.started_at_unix,
    }
}

pub fn dpi_status() -> DpiStatusPayload {
    #[cfg(windows)]
    {
        runtime_snapshot_blocking(DISCOVERY_TIMEOUT)
            .map(project_snapshot)
            // A busy/unreachable pipe is NOT a confirmed stop. Keep the last
            // authenticated generation so the user can still press Stop.
            .unwrap_or_else(|_| projection_status(&lock_projection()))
    }
    #[cfg(not(windows))]
    {
        inactive_status()
    }
}

fn projection_status(projection: &DpiProjection) -> DpiStatusPayload {
    DpiStatusPayload {
        active: projection.generation.is_some(),
        processes: Vec::new(),
        started_at: projection.started_at_unix,
    }
}

pub fn legacy_reliability_status(
    app: &AppHandle,
) -> crate::legacy_reliability::status::LegacyReliabilityStatus {
    #[cfg(windows)]
    {
        match runtime_snapshot_blocking(DISCOVERY_TIMEOUT) {
            Ok(snapshot) => {
                let (status, keep_monitoring) = legacy_monitor_projection(&snapshot);
                if keep_monitoring {
                    // Bootstrap/resume must attach to a service-owned runtime
                    // that survived an abnormal UI exit. Seeding `last`
                    // avoids immediately re-emitting the same bootstrap value.
                    spawn_legacy_reliability_monitor(app, Some(status.clone()));
                } else {
                    cancel_legacy_reliability_monitor();
                }
                status
            }
            Err(_) => {
                // A transient discovery failure must not cancel an already
                // running monitor. It will retry and only project inactive
                // after a bounded sequence of failures.
                crate::legacy_reliability::status::LegacyReliabilityStatus::inactive()
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        crate::legacy_reliability::status::LegacyReliabilityStatus::inactive()
    }
}

#[cfg(windows)]
fn project_legacy_reliability(
    snapshot: &RuntimeSnapshot,
) -> crate::legacy_reliability::status::LegacyReliabilityStatus {
    use crate::legacy_reliability::assessment::{
        AssessmentClassification, AssessmentConfidence, EvidenceSummary, LanePhase,
    };
    use crate::legacy_reliability::policy::PresumedIntent;
    use crate::legacy_reliability::recovery::{
        AssistedProposalView, IncidentId, ProposalId, RecoveryAttemptView, RecoveryCompletion,
        RecoveryDisposition, RecoveryOrigin,
    };
    use crate::legacy_reliability::status::{
        LegacyLaneStatus, LegacyReliabilityMode, LegacyReliabilityPhase, LegacyReliabilityStatus,
    };

    let running_applications = snapshot
        .legacy_access_activity
        .as_ref()
        .filter(|activity| activity.sensor_available)
        .map(|activity| {
            activity
                .running_categories
                .iter()
                .copied()
                .map(protocol_category_name)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let Some(reliability) = snapshot.legacy_reliability.as_ref() else {
        let mut status = LegacyReliabilityStatus::inactive();
        status.running_applications = running_applications;
        return status;
    };
    let active_configs = snapshot
        .dpi
        .as_ref()
        .map(|runtime| {
            runtime
                .selections
                .iter()
                .map(|selection| (selection.category, selection.strategy_id.as_str()))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let phase = match reliability.health {
        LegacyObserverHealth::Ready => LegacyReliabilityPhase::Observing,
        LegacyObserverHealth::Degraded => LegacyReliabilityPhase::Degraded,
        LegacyObserverHealth::Blind | LegacyObserverHealth::Stopped => {
            LegacyReliabilityPhase::Blind
        }
    };
    let lanes = reliability
        .lanes
        .iter()
        .map(|lane| {
            let classification = match lane.classification {
                ProtocolClassification::AwaitingEvidence => {
                    AssessmentClassification::AwaitingEvidence
                }
                ProtocolClassification::Working => AssessmentClassification::Working,
                ProtocolClassification::Offline => AssessmentClassification::Offline,
                ProtocolClassification::DnsFailure => AssessmentClassification::DnsFailure,
                ProtocolClassification::UpstreamDegraded => {
                    AssessmentClassification::UpstreamDegraded
                }
                ProtocolClassification::TargetUnavailable => {
                    AssessmentClassification::TargetUnavailable
                }
                ProtocolClassification::ServiceSlow => AssessmentClassification::ServiceSlow,
                ProtocolClassification::DpiSuspected => AssessmentClassification::DpiSuspected,
                ProtocolClassification::DpiBlocked => AssessmentClassification::DpiBlocked,
                ProtocolClassification::SensorUnreliable => {
                    AssessmentClassification::SensorUnreliable
                }
            };
            LegacyLaneStatus {
                category: protocol_category_name(lane.category).to_owned(),
                active_config: active_configs
                    .get(&lane.category)
                    .map(|value| (*value).to_owned()),
                lane_generation: lane.lane_generation,
                phase: match lane.phase {
                    ProtocolLanePhase::Observing => LanePhase::Observing,
                    ProtocolLanePhase::Healthy => LanePhase::Healthy,
                    ProtocolLanePhase::Suspect => LanePhase::Suspect,
                    ProtocolLanePhase::GatePending => LanePhase::GatePending,
                    ProtocolLanePhase::BlockedCooldown => LanePhase::BlockedCooldown,
                    ProtocolLanePhase::SensorUnreliable => LanePhase::SensorUnreliable,
                },
                classification,
                confidence: match lane.confidence {
                    ProtocolConfidence::None => AssessmentConfidence::None,
                    ProtocolConfidence::Low => AssessmentConfidence::Low,
                    ProtocolConfidence::Medium => AssessmentConfidence::Medium,
                    ProtocolConfidence::High => AssessmentConfidence::High,
                },
                evidence: EvidenceSummary {
                    working_flows: u16::from(lane.evidence.working_flows),
                    working_targets: u16::from(lane.evidence.working_targets),
                    reset_flows: u16::from(lane.evidence.reset_flows),
                    reset_targets: u16::from(lane.evidence.reset_targets),
                    blackhole_flows: u16::from(lane.evidence.blackhole_flows),
                    blackhole_targets: u16::from(lane.evidence.blackhole_targets),
                },
                working_confirmed_recently: lane.working_confirmed_recently
                    || matches!(classification, AssessmentClassification::Working),
                cooldown_until_ms: None,
            }
        })
        .collect();
    let presumed_reason = if phase == LegacyReliabilityPhase::Blind {
        AssessmentClassification::SensorUnreliable
    } else {
        AssessmentClassification::AwaitingEvidence
    };

    let proposal = reliability.recovery.proposal.as_ref().and_then(|proposal| {
        Some(AssistedProposalView {
            proposal_id: ProposalId::new(proposal.proposal_id)?,
            attempt_id: crate::legacy_reliability::contracts::AttemptId::new(proposal.attempt_id),
            incident_id: IncidentId::new(proposal.incident_id),
            category: protocol_category_name(proposal.category).to_owned(),
            previous_config_id: proposal.previous_config_id.clone(),
            candidate_config_id: proposal.candidate_config_id.clone(),
            expires_at_monotonic_ms: proposal.expires_at_monotonic_ms,
        })
    });
    let active_attempt =
        reliability
            .recovery
            .active_attempt
            .as_ref()
            .map(|attempt| RecoveryAttemptView {
                attempt_id: crate::legacy_reliability::contracts::AttemptId::new(
                    attempt.attempt_id,
                ),
                incident_id: IncidentId::new(attempt.incident_id),
                category: protocol_category_name(attempt.category).to_owned(),
                previous_config_id: attempt.previous_config_id.clone(),
                candidate_config_id: attempt.candidate_config_id.clone(),
                origin: match attempt.origin {
                    ProtocolRecoveryOrigin::Assisted => RecoveryOrigin::Assisted,
                    ProtocolRecoveryOrigin::Automatic { control_generation } => {
                        RecoveryOrigin::Automatic { control_generation }
                    }
                },
                phase: project_recovery_phase(attempt.phase),
                phase_started_at_monotonic_ms: attempt.phase_started_at_monotonic_ms,
            });
    let last_completion = reliability
        .recovery
        .last_completion
        .as_ref()
        .map(|completion| RecoveryCompletion {
            attempt_id: crate::legacy_reliability::contracts::AttemptId::new(completion.attempt_id),
            incident_id: IncidentId::new(completion.incident_id),
            category: protocol_category_name(completion.category).to_owned(),
            previous_config_id: completion.previous_config_id.clone(),
            candidate_config_id: completion.candidate_config_id.clone(),
            origin: match completion.origin {
                ProtocolRecoveryOrigin::Assisted => RecoveryOrigin::Assisted,
                ProtocolRecoveryOrigin::Automatic { control_generation } => {
                    RecoveryOrigin::Automatic { control_generation }
                }
            },
            phase: project_recovery_phase(completion.phase),
            disposition: match completion.disposition {
                LegacyRecoveryCompletionDisposition::CandidateApplied => {
                    RecoveryDisposition::CandidateApplied
                }
                LegacyRecoveryCompletionDisposition::PreviousPreserved => {
                    RecoveryDisposition::PreviousPreserved
                }
                LegacyRecoveryCompletionDisposition::RolledBack => RecoveryDisposition::RolledBack,
                LegacyRecoveryCompletionDisposition::ProcessFailed => {
                    RecoveryDisposition::ProcessFailed
                }
            },
            finished_at_monotonic_ms: completion.finished_at_monotonic_ms,
        });

    LegacyReliabilityStatus {
        mode: match reliability.recovery.controls.mode {
            ProtocolRecoveryMode::ObserveOnly => LegacyReliabilityMode::ObserveOnly,
            ProtocolRecoveryMode::Assisted => LegacyReliabilityMode::Assisted,
            ProtocolRecoveryMode::Automatic => LegacyReliabilityMode::Automatic,
        },
        phase,
        active_categories: reliability
            .active_categories
            .iter()
            .copied()
            .map(protocol_category_name)
            .map(str::to_owned)
            .collect(),
        running_applications,
        session_id: Some(reliability.session_id),
        sensor_generation: Some(reliability.sensor_generation),
        lanes,
        presumed_intent: PresumedIntent::Wait {
            reason: presumed_reason,
        },
        proposal,
        active_attempt,
        last_completion,
        negative_cooldown_count: usize::from(reliability.recovery.negative_cooldown_count),
        automatic_paused: reliability.recovery.controls.automatic_paused,
        automatic_pacing_remaining_ms: reliability.recovery.automatic_pacing_remaining_ms,
        frozen_categories: reliability
            .recovery
            .controls
            .frozen_categories
            .iter()
            .copied()
            .map(protocol_category_name)
            .map(str::to_owned)
            .collect(),
        halted_categories: reliability
            .recovery
            .halted_categories
            .iter()
            .copied()
            .map(protocol_category_name)
            .map(str::to_owned)
            .collect(),
    }
}

#[cfg(windows)]
const fn project_recovery_phase(
    phase: LegacyRecoveryAttemptPhase,
) -> crate::legacy_reliability::recovery::RecoveryPhase {
    use crate::legacy_reliability::recovery::RecoveryPhase;

    match phase {
        LegacyRecoveryAttemptPhase::Preflight => RecoveryPhase::Preflight,
        LegacyRecoveryAttemptPhase::Stopping => RecoveryPhase::Stopping,
        LegacyRecoveryAttemptPhase::Starting => RecoveryPhase::Starting,
        LegacyRecoveryAttemptPhase::Confirming => RecoveryPhase::Confirming,
        LegacyRecoveryAttemptPhase::RollingBack => RecoveryPhase::RollingBack,
        LegacyRecoveryAttemptPhase::Applied => RecoveryPhase::Applied,
        LegacyRecoveryAttemptPhase::ProcessFailed => RecoveryPhase::ProcessFailed,
    }
}

#[cfg(windows)]
const fn protocol_category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

fn emit_status(app: &AppHandle, payload: DpiStatusPayload) {
    let revision = app.state::<AppState>().dpi.lock_recover().bump_revision();
    let _ = app.emit("dpi-status", VersionedSection::new(revision, payload));
}

fn emit_legacy_reliability_status(
    app: &AppHandle,
    status: crate::legacy_reliability::status::LegacyReliabilityStatus,
) {
    let revision = app.state::<AppState>().legacy_reliability_revision.bump();
    let _ = app.emit(
        crate::legacy_reliability::status::STATUS_EVENT,
        VersionedSection::new(revision, status),
    );
}

#[cfg(windows)]
fn cancel_legacy_reliability_monitor() {
    LEGACY_MONITOR_EPOCH.fetch_add(1, Ordering::AcqRel);
}

#[cfg(windows)]
fn legacy_monitor_projection(
    snapshot: &RuntimeSnapshot,
) -> (
    crate::legacy_reliability::status::LegacyReliabilityStatus,
    bool,
) {
    let keep_monitoring = snapshot.legacy_access_activity.is_some()
        || (snapshot.dpi.is_some() && snapshot.legacy_reliability.is_some());
    let status = project_legacy_reliability(snapshot);
    (status, keep_monitoring)
}

#[cfg(windows)]
fn spawn_legacy_reliability_monitor(
    app: &AppHandle,
    initial: Option<crate::legacy_reliability::status::LegacyReliabilityStatus>,
) {
    let epoch = LEGACY_MONITOR_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = initial;
        let mut consecutive_failures = 0_u8;
        let mut reported_persistence_failure = None;
        loop {
            if LEGACY_MONITOR_EPOCH.load(Ordering::Acquire) != epoch {
                break;
            }
            let snapshot = tauri::async_runtime::spawn_blocking(|| {
                runtime_snapshot_blocking(DISCOVERY_TIMEOUT)
            })
            .await;
            if LEGACY_MONITOR_EPOCH.load(Ordering::Acquire) != epoch {
                break;
            }
            match snapshot {
                Ok(Ok(snapshot)) => {
                    consecutive_failures = 0;
                    let completion_attempt =
                        applied_legacy_recovery_config(&snapshot).map(|value| value.0);
                    // Settings persistence takes the synchronous save gate and
                    // performs fsync/rename. Calling it directly from this async
                    // monitor would make Tokio's `blocking_lock` panic; release
                    // builds use `panic = "abort"`, so accepting a candidate used
                    // to terminate the whole launcher. Keep the blocking boundary
                    // explicit, as in the other async settings mutations.
                    let save_app = app.clone();
                    let save_snapshot = snapshot.clone();
                    let reconciliation = tauri::async_runtime::spawn_blocking(move || {
                        reconcile_legacy_recovery_snapshot(&save_app, &save_snapshot)
                    })
                    .await
                    .map_err(|error| {
                        format!("не удалось завершить сохранение Legacy recovery: {error}")
                    })
                    .and_then(|result| result);
                    match reconciliation {
                        Ok(_) => reported_persistence_failure = None,
                        Err(error)
                            if completion_attempt.is_some()
                                && completion_attempt != reported_persistence_failure =>
                        {
                            util::emit_log(
                                &app,
                                "warn",
                                "dpi",
                                &format!(
                                    "Подтверждённая Legacy-конфигурация работает, но не сохранена: {error}"
                                ),
                            );
                            reported_persistence_failure = completion_attempt;
                        }
                        Err(_) => {}
                    }
                    let (status, keep_monitoring) = legacy_monitor_projection(&snapshot);
                    if last.as_ref() != Some(&status) {
                        emit_legacy_reliability_status(&app, status.clone());
                        last = Some(status);
                    }
                    if !keep_monitoring {
                        break;
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    if consecutive_failures >= LEGACY_MONITOR_FAILURES_BEFORE_INACTIVE {
                        let status =
                            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive();
                        if last.as_ref() != Some(&status) {
                            emit_legacy_reliability_status(&app, status.clone());
                            last = Some(status);
                        }
                    }
                }
            }
            tokio::time::sleep(LEGACY_MONITOR_INTERVAL).await;
        }
    });
}

#[cfg(windows)]
fn category(value: &str) -> Result<DpiCategory, String> {
    match value {
        "discord" => Ok(DpiCategory::Discord),
        "youtube_twitch" => Ok(DpiCategory::YoutubeTwitch),
        "gaming" => Ok(DpiCategory::Gaming),
        "atrisk" | "at_risk" => Ok(DpiCategory::AtRisk),
        "universal" => Ok(DpiCategory::Universal),
        _ => Err(format!("неизвестная DPI-категория: {value}")),
    }
}

#[cfg(windows)]
fn category_name(value: DpiCategory) -> &'static str {
    match value {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

#[cfg(windows)]
fn wire_override(
    candidate: &crate::adaptive_strategy::dsl::StrategyCandidate,
) -> Result<Zapret2AdaptiveOverride, String> {
    use crate::adaptive_strategy::dsl::{
        AllowedPayload, AllowedRange, StrategyFunction, StrategyTransport, StrategyValue,
    };
    let validation = crate::adaptive_strategy::validator::validate(candidate);
    if !validation.is_valid() {
        return Err(format!(
            "Adaptive candidate не прошёл локальную валидацию: {}",
            validation
                .errors
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let payload = match candidate.payload.as_slice() {
        [AllowedPayload::TlsClientHello] => Zapret2AdaptivePayload::TlsClientHello,
        [AllowedPayload::QuicInitial] => Zapret2AdaptivePayload::QuicInitial,
        _ => return Err("Adaptive candidate содержит недопустимый payload".into()),
    };
    let transport = match candidate.transport {
        StrategyTransport::Tls => Zapret2AdaptiveTransport::Tls,
        StrategyTransport::Quic => Zapret2AdaptiveTransport::Quic,
    };
    let steps = candidate
        .steps
        .iter()
        .map(|step| {
            let function = match step.function {
                StrategyFunction::Fake => Zapret2AdaptiveFunction::Fake,
                StrategyFunction::MultiSplit => Zapret2AdaptiveFunction::MultiSplit,
                StrategyFunction::MultiDisorder => Zapret2AdaptiveFunction::MultiDisorder,
                StrategyFunction::MultiDisorderLegacy => {
                    Zapret2AdaptiveFunction::MultiDisorderLegacy
                }
                StrategyFunction::FakeDSplit => Zapret2AdaptiveFunction::FakeDSplit,
                StrategyFunction::FakeDDisorder => Zapret2AdaptiveFunction::FakeDDisorder,
                StrategyFunction::SendIpFrag => Zapret2AdaptiveFunction::SendIpFrag,
                StrategyFunction::Drop => Zapret2AdaptiveFunction::Drop,
            };
            let args = step
                .args
                .iter()
                .map(|(key, value)| {
                    let value = match value {
                        StrategyValue::Bool(value) => Zapret2AdaptiveValue::Bool(*value),
                        StrategyValue::Integer(value) => Zapret2AdaptiveValue::Integer(*value),
                        StrategyValue::Text(value) => Zapret2AdaptiveValue::Text(value.clone()),
                    };
                    (key.clone(), value)
                })
                .collect();
            Zapret2AdaptiveStep { function, args }
        })
        .collect();
    let candidate = Zapret2AdaptiveOverride {
        schema_version: ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
        category: category(candidate.category.as_key())?,
        transport,
        steps,
        payload,
        out_range: candidate.out_range.map(|value| match value {
            AllowedRange::FirstTenDataPackets => Zapret2AdaptiveRange::FirstTenDataPackets,
            AllowedRange::Always => Zapret2AdaptiveRange::Always,
        }),
    };
    candidate
        .validate()
        .map_err(|error| format!("Adaptive IPC candidate отклонён: {error}"))?;
    Ok(candidate)
}

#[cfg(windows)]
fn wire_overrides(
    overrides: &BTreeMap<String, crate::adaptive_strategy::dsl::StrategyCandidate>,
) -> Result<Vec<Zapret2AdaptiveOverride>, String> {
    overrides
        .iter()
        .map(|(key, candidate)| {
            let expected = crate::adaptive_strategy::dsl::override_key(
                candidate.category,
                candidate.transport,
            );
            if key != &expected {
                return Err("Adaptive override cache key не совпадает с candidate".into());
            }
            wire_override(candidate)
        })
        .collect()
}

#[cfg(windows)]
fn start_request(
    engine: DpiEngine,
    selections: &[(String, String)],
    zapret2_level: u8,
    legacy_reliability: bool,
    adaptive_overrides: &BTreeMap<String, crate::adaptive_strategy::dsl::StrategyCandidate>,
) -> Result<Request, String> {
    if engine != DpiEngine::Zapret2 && !adaptive_overrides.is_empty() {
        return Err("Adaptive overrides допустимы только для Zapret2".into());
    }
    let selections = match engine {
        DpiEngine::Legacy => selections
            .iter()
            .map(|(category_name, strategy_id)| {
                if strategy_id.is_empty() {
                    return Err(format!(
                        "для категории {category_name} не выбрана стратегия"
                    ));
                }
                Ok(DpiSelection {
                    category: category(category_name)?,
                    strategy_id: strategy_id.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        DpiEngine::Zapret2 => selections
            .iter()
            .map(|(category_name, _)| {
                let category = category(category_name)?;
                if !matches!(
                    category,
                    DpiCategory::Discord | DpiCategory::YoutubeTwitch | DpiCategory::Gaming
                ) {
                    return Err(format!("Zapret2 не поддерживает категорию {category_name}"));
                }
                Ok(DpiSelection {
                    category,
                    strategy_id: format!("builtin-{}", category_name.replace('_', "-")),
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    };
    Ok(Request::DpiStart(DpiStartRequest {
        engine,
        selections,
        options: DpiRuntimeOptions {
            zapret2_level,
            legacy_reliability,
            zapret2_overrides: wire_overrides(adaptive_overrides)?,
        },
    }))
}

pub async fn dpi_start(
    app: &AppHandle,
    engine: DpiEngine,
    selections: Vec<(String, String)>,
    zapret2_level: u8,
) -> Result<u64, String> {
    #[cfg(windows)]
    {
        let adaptive_overrides = if engine == DpiEngine::Zapret2 {
            crate::adaptive_strategy::runtime::confirmed_overrides_for_current_network(
                app,
                &selections,
            )
            .await
        } else {
            BTreeMap::new()
        };
        let reliability_requested = engine == DpiEngine::Legacy
            && app
                .state::<AppState>()
                .settings
                .lock_recover()
                .legacy_reliability_enabled;
        let request_selections = selections.clone();
        let request_overrides = adaptive_overrides.clone();
        let (response, reliability_enabled) = tauri::async_runtime::spawn_blocking(move || {
            let service_capabilities = capabilities()?;
            let reliability_available = engine == DpiEngine::Legacy
                && service_capabilities.features.contains(&Feature::EyesEvents);
            let reliability_enabled = reliability_requested && reliability_available;
            if !request_overrides.is_empty()
                && !service_capabilities
                    .features
                    .contains(&Feature::DpiZapret2Adaptive)
            {
                return Err("Защищённая служба не поддерживает Adaptive Zapret2".into());
            }
            let request = start_request(
                engine,
                &request_selections,
                zapret2_level,
                reliability_enabled,
                &request_overrides,
            )?;
            Ok::<_, String>((
                call_service("dpi-start", request, OPERATION_TIMEOUT)?,
                reliability_enabled,
            ))
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        let generation = match response {
            Response::Started(started) => started.generation,
            _ => return Err("защищённая служба вернула неожиданный ответ DPI start".to_string()),
        };
        {
            let mut projection = lock_projection();
            projection.generation = Some(generation);
            projection.engine = Some(engine);
            projection.selections = selections;
            projection.zapret2_level = (engine == DpiEngine::Zapret2).then_some(zapret2_level);
            projection.adaptive_overrides = adaptive_overrides;
            projection.started_at_unix = Some(util::unix_secs());
        }
        emit_status(
            app,
            DpiStatusPayload {
                active: true,
                processes: Vec::new(),
                started_at: lock_projection().started_at_unix,
            },
        );
        if reliability_enabled {
            spawn_legacy_reliability_monitor(app, None);
        } else {
            cancel_legacy_reliability_monitor();
            emit_legacy_reliability_status(
                app,
                crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
            );
        }
        util::emit_log(
            app,
            "success",
            "dpi",
            "DPI запущен защищённой системной службой.",
        );
        Ok(generation)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, engine, selections, zapret2_level);
        Err(unavailable())
    }
}

/// Exact process-local spec for Adaptive generation fencing. A service-owned
/// generation discovered after UI restart deliberately has no replaceable
/// Zapret2 spec because the sanitized snapshot omits level and overrides.
pub(crate) fn adaptive_runtime_snapshot() -> DpiRuntimeSnapshot {
    let projection = lock_projection();
    let generation = projection.generation.unwrap_or(0);
    let launch = match projection.engine {
        Some(DpiEngine::Legacy) if generation != 0 => Some(DpiLaunchSpec::Legacy {
            selections: projection.selections.clone(),
        }),
        Some(DpiEngine::Zapret2) if generation != 0 && projection.zapret2_level.is_some() => {
            Some(DpiLaunchSpec::Zapret2 {
                selections: projection.selections.clone(),
                adaptive_overrides: projection.adaptive_overrides.clone(),
            })
        }
        _ => None,
    };
    DpiRuntimeSnapshot { generation, launch }
}

#[cfg(windows)]
fn restored_zapret2_projection(
    snapshot: &RuntimeSnapshot,
    original: &DpiRuntimeSnapshot,
    zapret2_level: u8,
    started_at_unix: Option<u64>,
) -> Option<DpiProjection> {
    let runtime = snapshot.dpi.as_ref()?;
    let DpiLaunchSpec::Zapret2 {
        selections,
        adaptive_overrides,
    } = original.launch.as_ref()?
    else {
        return None;
    };
    if runtime.engine != DpiEngine::Zapret2
        || runtime.selections.len() != selections.len()
        || !runtime
            .selections
            .iter()
            .zip(selections)
            .all(|(observed, (category, _))| category_name(observed.category) == category)
    {
        return None;
    }
    Some(DpiProjection {
        generation: Some(runtime.generation),
        engine: Some(DpiEngine::Zapret2),
        selections: selections.clone(),
        zapret2_level: Some(zapret2_level),
        adaptive_overrides: adaptive_overrides.clone(),
        started_at_unix,
    })
}

fn runtime_already_matches_snapshot(
    current: &DpiRuntimeSnapshot,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
) -> bool {
    current.generation == expected_generation && current.launch == original.launch
}

pub(crate) async fn replace_zapret2_overrides(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
    adaptive_overrides: &BTreeMap<String, crate::adaptive_strategy::dsl::StrategyCandidate>,
) -> Result<u64, String> {
    #[cfg(windows)]
    {
        let DpiLaunchSpec::Zapret2 { selections, .. } = original
            .launch
            .as_ref()
            .ok_or_else(|| "Adaptive snapshot не содержит Zapret2".to_string())?
        else {
            return Err("Adaptive replace допустим только для Zapret2".into());
        };
        let zapret2_level = {
            let projection = lock_projection();
            if projection.generation != Some(expected_generation)
                || projection.engine != Some(DpiEngine::Zapret2)
                || projection.selections != *selections
            {
                return Err(format!(
                    "DPI runtime изменён другой операцией: expected generation {expected_generation}"
                ));
            }
            projection.zapret2_level.ok_or_else(|| {
                "Adaptive replace недоступен для runtime, найденного после перезапуска UI"
                    .to_string()
            })?
        };
        let Request::DpiStart(runtime) = start_request(
            DpiEngine::Zapret2,
            selections,
            zapret2_level,
            false,
            adaptive_overrides,
        )?
        else {
            return Err("Не удалось построить защищённый Zapret2 request".into());
        };
        let request = Request::DpiReplace(DpiReplaceRequest {
            expected_generation,
            runtime,
        });
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service_for_replace("dpi-replace", request, OPERATION_TIMEOUT)
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))?;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                // RuntimeFailed is returned after the service tried the new
                // plan and then synchronously restored the previous one. The
                // public snapshot omits its level and adaptive DSL, but the
                // exact previous spec is still available in `original`.
                if let Ok(snapshot) = tauri::async_runtime::spawn_blocking(|| {
                    runtime_snapshot_blocking(OPERATION_TIMEOUT)
                })
                .await
                .map_err(|join_error| join_error.to_string())?
                {
                    let started_at_unix = lock_projection().started_at_unix;
                    let restored = (error.code == Some(ServiceErrorCode::RuntimeFailed))
                        .then(|| {
                            restored_zapret2_projection(
                                &snapshot,
                                original,
                                zapret2_level,
                                started_at_unix,
                            )
                        })
                        .flatten();
                    let status = if let Some(restored) = restored {
                        let mut projection = lock_projection();
                        if projection.generation == Some(expected_generation) {
                            *projection = restored;
                            DpiStatusPayload {
                                active: true,
                                processes: Vec::new(),
                                started_at: projection.started_at_unix,
                            }
                        } else {
                            drop(projection);
                            project_snapshot(snapshot)
                        }
                    } else {
                        project_snapshot(snapshot)
                    };
                    emit_status(app, status);
                }
                return Err(error.message);
            }
        };
        let generation = match response {
            Response::Started(started) => started.generation,
            _ => return Err("Защищённая служба вернула неожиданный ответ DPI replace".to_string()),
        };
        {
            let mut projection = lock_projection();
            if projection.generation != Some(expected_generation) {
                return Err("Локальная DPI generation изменилась во время replace".into());
            }
            projection.generation = Some(generation);
            projection.adaptive_overrides = adaptive_overrides.clone();
        }
        emit_status(
            app,
            DpiStatusPayload {
                active: true,
                processes: Vec::new(),
                started_at: lock_projection().started_at_unix,
            },
        );
        Ok(generation)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, original, expected_generation, adaptive_overrides);
        Err(unavailable())
    }
}

pub(crate) async fn start_adaptive_candidate(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
    category_name: &str,
    candidate: crate::adaptive_strategy::dsl::StrategyCandidate,
) -> Result<u64, String> {
    let DpiLaunchSpec::Zapret2 {
        selections,
        adaptive_overrides,
    } = original
        .launch
        .as_ref()
        .ok_or_else(|| "DPI runtime остановлен — adaptive search недоступен".to_string())?
    else {
        return Err("Adaptive search работает только поверх активного Zapret2".into());
    };
    if candidate.category.as_key() != category_name
        || !selections
            .iter()
            .any(|(selected_category, _)| selected_category == category_name)
    {
        return Err("Категория adaptive candidate не совпадает с активным runtime".into());
    }
    let mut overrides = adaptive_overrides.clone();
    overrides.insert(
        crate::adaptive_strategy::dsl::override_key(candidate.category, candidate.transport),
        candidate,
    );
    replace_zapret2_overrides(app, original, expected_generation, &overrides).await
}

pub(crate) async fn restore_adaptive_snapshot(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
) -> Result<u64, String> {
    let DpiLaunchSpec::Zapret2 {
        adaptive_overrides, ..
    } = original
        .launch
        .as_ref()
        .ok_or_else(|| "Adaptive rollback snapshot отсутствует".to_string())?
    else {
        return Err("Adaptive rollback поддерживает только Zapret2".into());
    };
    let current = adaptive_runtime_snapshot();
    if runtime_already_matches_snapshot(&current, original, expected_generation) {
        return Ok(current.generation);
    }
    replace_zapret2_overrides(app, original, expected_generation, adaptive_overrides).await
}

pub async fn dpi_active() -> Result<bool, String> {
    #[cfg(windows)]
    {
        let snapshot =
            tauri::async_runtime::spawn_blocking(|| runtime_snapshot_blocking(OPERATION_TIMEOUT))
                .await
                .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        Ok(snapshot.dpi.is_some())
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}

pub async fn dpi_stop(app: &AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        let snapshot =
            tauri::async_runtime::spawn_blocking(|| runtime_snapshot_blocking(OPERATION_TIMEOUT))
                .await
                .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        let Some(runtime) = snapshot.dpi else {
            cancel_legacy_reliability_monitor();
            *lock_projection() = DpiProjection::default();
            emit_status(app, inactive_status());
            emit_legacy_reliability_status(
                app,
                crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
            );
            return Ok(());
        };
        let request = Request::DpiStop(DpiStopRequest {
            generation: runtime.generation,
        });
        let response = tauri::async_runtime::spawn_blocking(move || {
            call_service("dpi-stop", request, OPERATION_TIMEOUT)
        })
        .await
        .map_err(|error| format!("runtime-client завершился с ошибкой: {error}"))??;
        if !matches!(response, Response::Stopped) {
            return Err("защищённая служба вернула неожиданный ответ DPI stop".to_string());
        }
        cancel_legacy_reliability_monitor();
        *lock_projection() = DpiProjection::default();
        emit_status(app, inactive_status());
        emit_legacy_reliability_status(
            app,
            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive(),
        );
        util::emit_log(
            app,
            "info",
            "dpi",
            "DPI остановлен защищённой системной службой.",
        );
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    #[cfg(windows)]
    #[test]
    fn unavailable_pipe_preserves_the_last_confirmed_generation() {
        let active = DpiProjection {
            generation: Some(7),
            started_at_unix: Some(123),
            ..Default::default()
        };
        let status = projection_status(&active);
        assert!(status.active);
        assert_eq!(status.started_at, Some(123));
        assert!(!projection_status(&DpiProjection::default()).active);
    }

    #[cfg(windows)]
    #[test]
    fn service_conflicts_are_described_in_the_operation_domain() {
        assert_eq!(
            service_error_message("hosts-install", ServiceErrorCode::Conflict),
            "файл hosts изменился прямо во время операции; повторите попытку"
        );
        assert_eq!(
            service_error_message("proxy-lan-open", ServiceErrorCode::Conflict),
            "состояние доступа к proxy изменилось; повторите операцию"
        );
        assert_eq!(
            service_error_message("dpi-start", ServiceErrorCode::Conflict),
            "состояние DPI изменилось; повторите операцию"
        );
    }

    #[cfg(windows)]
    #[test]
    fn capability_projection_preserves_independent_feature_gates() {
        let projected = project_capabilities(Capabilities {
            service_version: "1.2.3".into(),
            features: vec![
                Feature::Dpi,
                Feature::DpiZapret2,
                Feature::DpiZapret2Adaptive,
                Feature::EyesEvents,
                Feature::LegacyReliabilityControls,
                Feature::LegacyReliability,
                Feature::Hosts,
            ],
        });

        assert!(projected.service_available);
        assert_eq!(projected.service_version.as_deref(), Some("1.2.3"));
        assert!(projected.dpi);
        assert!(projected.zapret2);
        assert!(projected.adaptive_zapret2);
        assert!(projected.eyes_events);
        assert!(projected.legacy_reliability_controls);
        assert!(projected.legacy_reliability);
        assert!(projected.hosts);
        assert!(!projected.proxy_lan_firewall);

        let inconsistent = project_capabilities(Capabilities {
            service_version: "1.2.3".into(),
            features: vec![Feature::DpiZapret2Adaptive, Feature::LegacyReliability],
        });
        assert!(!inconsistent.adaptive_zapret2);
        assert!(!inconsistent.legacy_reliability);
    }

    #[cfg(windows)]
    #[test]
    fn maps_only_allowlisted_categories_and_strategy_ids() {
        let request = start_request(
            DpiEngine::Legacy,
            &[("discord".into(), "discord_1.conf".into())],
            0,
            true,
            &BTreeMap::new(),
        )
        .expect("valid protected request");
        let Request::DpiStart(request) = request else {
            panic!("expected DPI start request");
        };
        assert_eq!(request.engine, DpiEngine::Legacy);
        assert_eq!(request.selections[0].category, DpiCategory::Discord);
        assert_eq!(request.selections[0].strategy_id, "discord_1.conf");
        assert!(request.options.legacy_reliability);

        assert!(start_request(
            DpiEngine::Legacy,
            &[("unknown".into(), "x.conf".into())],
            0,
            false,
            &BTreeMap::new(),
        )
        .is_err());
        assert!(start_request(
            DpiEngine::Legacy,
            &[("discord".into(), String::new())],
            0,
            false,
            &BTreeMap::new(),
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn zapret2_request_exposes_only_fixed_category_group_ids() {
        let request = start_request(
            DpiEngine::Zapret2,
            &[
                ("discord".into(), String::new()),
                ("youtube_twitch".into(), "ignored.conf".into()),
            ],
            2,
            false,
            &BTreeMap::new(),
        )
        .expect("valid protected Zapret2 request");
        let Request::DpiStart(request) = request else {
            panic!("expected DPI start request");
        };
        assert_eq!(request.engine, DpiEngine::Zapret2);
        assert_eq!(request.options.zapret2_level, 2);
        assert!(!request.options.legacy_reliability);
        assert_eq!(
            request
                .selections
                .iter()
                .map(|selection| selection.strategy_id.as_str())
                .collect::<Vec<_>>(),
            ["builtin-discord", "builtin-youtube-twitch"]
        );
        assert!(start_request(
            DpiEngine::Zapret2,
            &[("universal".into(), String::new())],
            0,
            false,
            &BTreeMap::new(),
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn adaptive_candidate_crosses_ipc_as_a_typed_allowlisted_override() {
        use crate::adaptive_strategy::dsl::{
            override_key, AdaptiveCategory, AllowedPayload, AllowedRange, StrategyCandidate,
            StrategyFunction, StrategyStep, StrategyTransport, StrategyValue,
        };

        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::MultiDisorderLegacy)
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        );
        let key = override_key(candidate.category, candidate.transport);
        let overrides = BTreeMap::from([(key.clone(), candidate.clone())]);
        let Request::DpiStart(request) = start_request(
            DpiEngine::Zapret2,
            &[("discord".into(), String::new())],
            1,
            false,
            &overrides,
        )
        .unwrap() else {
            panic!("expected DPI start request");
        };

        let wire = request.options.zapret2_overrides.first().unwrap();
        assert_eq!(wire.schema_version, ZAPRET2_ADAPTIVE_SCHEMA_VERSION);
        assert_eq!(wire.category, DpiCategory::Discord);
        assert_eq!(wire.transport, Zapret2AdaptiveTransport::Tls);
        assert_eq!(wire.payload, Zapret2AdaptivePayload::TlsClientHello);
        assert_eq!(
            wire.out_range,
            Some(Zapret2AdaptiveRange::FirstTenDataPackets)
        );
        assert_eq!(
            wire.steps[0].function,
            Zapret2AdaptiveFunction::MultiDisorderLegacy
        );
        assert_eq!(
            wire.steps[0].args.get("pos"),
            Some(&Zapret2AdaptiveValue::Text("1,midsld".into()))
        );

        let mismatched_key = BTreeMap::from([("discord:quic".into(), candidate)]);
        assert!(start_request(
            DpiEngine::Zapret2,
            &[("discord".into(), String::new())],
            1,
            false,
            &mismatched_key,
        )
        .is_err());
        assert!(start_request(
            DpiEngine::Legacy,
            &[("discord".into(), "discord_1.conf".into())],
            0,
            false,
            &overrides,
        )
        .is_err());
        assert_eq!(key, "discord:tls");
    }

    #[cfg(windows)]
    #[test]
    fn attached_zapret2_projection_is_observable_but_adaptive_replace_fails_closed() {
        *lock_projection() = DpiProjection::default();
        let status = project_snapshot(RuntimeSnapshot {
            dpi: Some(obsession_runtime_protocol::DpiRuntimeSnapshot {
                generation: 41,
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "builtin-discord".into(),
                }],
            }),
            legacy_reliability: None,
            legacy_access_activity: None,
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        });

        assert!(status.active);
        assert!(status.processes.is_empty());
        let adaptive = adaptive_runtime_snapshot();
        assert_eq!(adaptive.generation, 41);
        assert!(adaptive.launch.is_none());
        let projection = lock_projection();
        assert_eq!(projection.engine, Some(DpiEngine::Zapret2));
        assert_eq!(projection.zapret2_level, None);
        assert!(projection.adaptive_overrides.is_empty());
        drop(projection);
        *lock_projection() = DpiProjection::default();
    }

    #[cfg(windows)]
    #[test]
    fn failed_replace_recovers_exact_projection_after_service_autorollback() {
        let original = DpiRuntimeSnapshot {
            generation: 40,
            launch: Some(DpiLaunchSpec::Zapret2 {
                selections: vec![("discord".into(), String::new())],
                adaptive_overrides: BTreeMap::new(),
            }),
        };
        let snapshot = RuntimeSnapshot {
            dpi: Some(obsession_runtime_protocol::DpiRuntimeSnapshot {
                generation: 42,
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "builtin-discord".into(),
                }],
            }),
            legacy_reliability: None,
            legacy_access_activity: None,
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        };

        let restored = restored_zapret2_projection(&snapshot, &original, 2, Some(123))
            .expect("the service restored the exact selected Zapret2 categories");
        assert_eq!(restored.generation, Some(42));
        assert_eq!(restored.zapret2_level, Some(2));
        assert_eq!(restored.selections, vec![("discord".into(), String::new())]);
        assert_eq!(restored.started_at_unix, Some(123));

        let current = DpiRuntimeSnapshot {
            generation: restored.generation.unwrap(),
            launch: Some(DpiLaunchSpec::Zapret2 {
                selections: restored.selections,
                adaptive_overrides: restored.adaptive_overrides,
            }),
        };
        assert!(runtime_already_matches_snapshot(&current, &original, 42));
        assert!(!runtime_already_matches_snapshot(&current, &original, 41));
    }

    #[cfg(windows)]
    #[test]
    fn failed_replace_does_not_trust_a_different_service_runtime() {
        let original = DpiRuntimeSnapshot {
            generation: 40,
            launch: Some(DpiLaunchSpec::Zapret2 {
                selections: vec![("discord".into(), String::new())],
                adaptive_overrides: BTreeMap::new(),
            }),
        };
        let snapshot = RuntimeSnapshot {
            dpi: Some(obsession_runtime_protocol::DpiRuntimeSnapshot {
                generation: 42,
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: DpiCategory::YoutubeTwitch,
                    strategy_id: "builtin-youtube-twitch".into(),
                }],
            }),
            legacy_reliability: None,
            legacy_access_activity: None,
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        };

        assert!(restored_zapret2_projection(&snapshot, &original, 2, Some(123)).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn settings_map_to_one_atomic_legacy_controls_request() {
        let mut settings = crate::settings::Settings::default();
        settings.legacy_reliability_enabled = true;
        settings.legacy_reliability_mode = "automatic".into();
        settings.legacy_automatic_paused = false;
        settings.legacy_reliability_frozen_categories = vec!["gaming".into(), "discord".into()];

        let Request::SetLegacyRecoveryControls(request) =
            legacy_controls_request(&settings).unwrap()
        else {
            panic!("expected atomic Legacy controls request");
        };
        assert_eq!(request.mode, ProtocolRecoveryMode::Automatic);
        assert!(!request.automatic_paused);
        assert_eq!(
            request.frozen_categories,
            [DpiCategory::Gaming, DpiCategory::Discord]
        );

        settings.legacy_reliability_enabled = false;
        let Request::SetLegacyRecoveryControls(disabled) =
            legacy_controls_request(&settings).unwrap()
        else {
            panic!("expected disabled Legacy controls request");
        };
        assert_eq!(disabled.mode, ProtocolRecoveryMode::ObserveOnly);
        assert!(disabled.automatic_paused);
    }

    #[cfg(windows)]
    #[test]
    fn projects_only_the_sanitized_service_reliability_status() {
        use obsession_runtime_protocol::{
            DpiRuntimeSnapshot, LegacyAccessActivitySnapshot, LegacyAssessmentClassification,
            LegacyAssessmentConfidence, LegacyEvidenceSnapshot, LegacyLanePhase,
            LegacyLaneRuntimeSnapshot, LegacyRecoveryCompletionSnapshot,
            LegacyRecoveryControlsSnapshot, LegacyRecoveryOrigin, LegacyRecoveryProposalSnapshot,
            LegacyRecoveryRuntimeSnapshot, LegacyReliabilityCounters,
            LegacyReliabilityRuntimeSnapshot,
        };

        let mut snapshot = RuntimeSnapshot {
            dpi: Some(DpiRuntimeSnapshot {
                generation: 7,
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
            }),
            legacy_reliability: Some(LegacyReliabilityRuntimeSnapshot {
                generation: 7,
                revision: 5,
                session_id: 11,
                sensor_generation: 13,
                registry_version: 17,
                health: LegacyObserverHealth::Ready,
                active_categories: vec![DpiCategory::Discord],
                lanes: vec![LegacyLaneRuntimeSnapshot {
                    category: DpiCategory::Discord,
                    lane_generation: 19,
                    phase: LegacyLanePhase::Healthy,
                    classification: LegacyAssessmentClassification::Working,
                    confidence: LegacyAssessmentConfidence::High,
                    working_confirmed_recently: true,
                    evidence: LegacyEvidenceSnapshot {
                        working_flows: 2,
                        working_targets: 2,
                        ..LegacyEvidenceSnapshot::default()
                    },
                }],
                counters: LegacyReliabilityCounters::default(),
                recovery: LegacyRecoveryRuntimeSnapshot {
                    phase: obsession_runtime_protocol::LegacyRecoveryPhase::Idle,
                    disposition: obsession_runtime_protocol::LegacyRecoveryDisposition::ObserveOnly,
                    controls: LegacyRecoveryControlsSnapshot {
                        control_generation: 23,
                        mode: ProtocolRecoveryMode::Automatic,
                        automatic_paused: false,
                        frozen_categories: vec![DpiCategory::Discord],
                    },
                    proposal: None,
                    active_attempt: None,
                    last_completion: None,
                    automatic_pacing_remaining_ms: None,
                    halted_categories: Vec::new(),
                    negative_cooldown_count: 0,
                },
            }),
            legacy_access_activity: Some(LegacyAccessActivitySnapshot {
                revision: 3,
                sensor_available: true,
                running_categories: vec![DpiCategory::Discord],
            }),
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        };

        let projected = project_legacy_reliability(&snapshot);
        assert_eq!(
            projected.phase,
            crate::legacy_reliability::status::LegacyReliabilityPhase::Observing
        );
        assert_eq!(projected.session_id, Some(11));
        assert_eq!(projected.sensor_generation, Some(13));
        assert_eq!(projected.active_categories, ["discord"]);
        assert_eq!(projected.running_applications, ["discord"]);
        assert_eq!(
            projected.lanes[0].active_config.as_deref(),
            Some("discord_1.conf")
        );
        assert!(projected.lanes[0].working_confirmed_recently);
        assert_eq!(
            projected.mode,
            crate::legacy_reliability::status::LegacyReliabilityMode::Automatic
        );
        assert!(!projected.automatic_paused);
        assert_eq!(projected.frozen_categories, ["discord"]);
        assert!(projected.proposal.is_none());
        assert!(projected.active_attempt.is_none());

        let recovery = &mut snapshot.legacy_reliability.as_mut().unwrap().recovery;
        recovery.phase = obsession_runtime_protocol::LegacyRecoveryPhase::Evaluating;
        recovery.disposition = obsession_runtime_protocol::LegacyRecoveryDisposition::Pending;
        recovery.controls.mode = ProtocolRecoveryMode::Assisted;
        recovery.controls.automatic_paused = true;
        recovery.proposal = Some(LegacyRecoveryProposalSnapshot {
            proposal_id: 29,
            attempt_id: 31,
            incident_id: 37,
            category: DpiCategory::Discord,
            previous_config_id: "discord_1.conf".into(),
            candidate_config_id: "discord_2.conf".into(),
            expires_at_monotonic_ms: 41,
        });
        let assisted = project_legacy_reliability(&snapshot);
        let proposal = assisted.proposal.unwrap();
        assert_eq!(
            assisted.mode,
            crate::legacy_reliability::status::LegacyReliabilityMode::Assisted
        );
        assert_eq!(proposal.proposal_id.get(), 29);
        assert_eq!(proposal.attempt_id.get(), 31);
        assert_eq!(proposal.incident_id.get(), 37);
        assert_eq!(proposal.category, "discord");
        assert_eq!(proposal.candidate_config_id, "discord_2.conf");

        let completion = LegacyRecoveryCompletionSnapshot {
            attempt_id: 31,
            incident_id: 37,
            category: DpiCategory::Discord,
            previous_config_id: "discord_1.conf".into(),
            candidate_config_id: "discord_2.conf".into(),
            origin: LegacyRecoveryOrigin::Automatic {
                control_generation: 23,
            },
            phase: LegacyRecoveryAttemptPhase::Applied,
            disposition: LegacyRecoveryCompletionDisposition::CandidateApplied,
            finished_at_monotonic_ms: 43,
        };
        snapshot
            .legacy_reliability
            .as_mut()
            .unwrap()
            .recovery
            .last_completion = Some(completion);
        snapshot.dpi.as_mut().unwrap().selections[0].strategy_id = "discord_2.conf".into();
        assert_eq!(
            applied_legacy_recovery_config(&snapshot),
            Some((31, "discord", "discord_2.conf"))
        );

        snapshot.dpi.as_mut().unwrap().selections[0].strategy_id = "discord_1.conf".into();
        assert!(applied_legacy_recovery_config(&snapshot).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn monitor_follows_recovery_generation_and_stops_only_without_runtime() {
        use obsession_runtime_protocol::{
            DpiRuntimeSnapshot, LegacyAccessActivitySnapshot, LegacyRecoveryControlsSnapshot,
            LegacyRecoveryRuntimeSnapshot, LegacyReliabilityCounters,
            LegacyReliabilityRuntimeSnapshot,
        };

        let mut snapshot = RuntimeSnapshot {
            dpi: Some(DpiRuntimeSnapshot {
                generation: 7,
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
            }),
            legacy_reliability: Some(LegacyReliabilityRuntimeSnapshot {
                generation: 7,
                revision: 1,
                session_id: 3,
                sensor_generation: 5,
                registry_version: 7,
                health: LegacyObserverHealth::Ready,
                active_categories: vec![DpiCategory::Discord],
                lanes: Vec::new(),
                counters: LegacyReliabilityCounters::default(),
                recovery: LegacyRecoveryRuntimeSnapshot {
                    phase: obsession_runtime_protocol::LegacyRecoveryPhase::Idle,
                    disposition: obsession_runtime_protocol::LegacyRecoveryDisposition::ObserveOnly,
                    controls: LegacyRecoveryControlsSnapshot {
                        control_generation: 1,
                        mode: ProtocolRecoveryMode::ObserveOnly,
                        automatic_paused: true,
                        frozen_categories: Vec::new(),
                    },
                    proposal: None,
                    active_attempt: None,
                    last_completion: None,
                    automatic_pacing_remaining_ms: None,
                    halted_categories: Vec::new(),
                    negative_cooldown_count: 0,
                },
            }),
            legacy_access_activity: None,
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        };

        let (initial, keep_monitoring) = legacy_monitor_projection(&snapshot);
        assert!(keep_monitoring);
        assert_eq!(
            initial.phase,
            crate::legacy_reliability::status::LegacyReliabilityPhase::Observing
        );

        // Stop/start inside recovery deliberately replaces the protected DPI
        // generation. The same monitor must continue with the new exact pair.
        snapshot.dpi.as_mut().unwrap().generation = 11;
        snapshot.legacy_reliability.as_mut().unwrap().generation = 11;
        let (after_recovery_restart, keep_monitoring) = legacy_monitor_projection(&snapshot);
        assert!(keep_monitoring);
        assert_eq!(after_recovery_restart.session_id, Some(3));

        snapshot.dpi = None;
        snapshot.legacy_reliability = None;
        snapshot.legacy_access_activity = Some(LegacyAccessActivitySnapshot {
            revision: 3,
            sensor_available: true,
            running_categories: vec![DpiCategory::Discord],
        });
        let (inactive, keep_monitoring) = legacy_monitor_projection(&snapshot);
        assert!(keep_monitoring);
        assert_eq!(inactive.running_applications, ["discord"]);

        snapshot.legacy_access_activity = None;
        let (inactive, keep_monitoring) = legacy_monitor_projection(&snapshot);
        assert!(!keep_monitoring);
        assert_eq!(
            inactive,
            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive()
        );
    }
}
