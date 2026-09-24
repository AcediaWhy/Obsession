//! Backend-authoritative first-run transaction for Onboarding V2.
//!
//! The frontend may edit a bounded draft and select a backend-built plan. It
//! never supplies executable paths, privileged arguments or rollback data.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use obsession_runtime_protocol::{AiRouteHealth, AiRouteKind, AiService};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::hosts::Provider;
use crate::settings::Settings;
use crate::state::AppState;
use crate::util::LockExt;

const FLOW_VERSION: u32 = 2;
const STATE_FILE: &str = "onboarding.json";
const LOG_FILE: &str = "onboarding.log";
const REPAIR_SETUP: &str = r"C:\Program Files\Obsession\uninstall.exe";
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const CONTROL_PROBE_URLS: &[&str] = &[
    "https://www.msftconnecttest.com/connecttest.txt",
    "https://www.google.com/generate_204",
    "https://example.com/",
];
const DISCORD_PROBE_URLS: &[&str] = &[
    "https://discord.com/api/v10/gateway",
    "https://cdn.discordapp.com/",
    "https://discord.com/",
];
static COMMAND_GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn command_gate() -> &'static tokio::sync::Mutex<()> {
    COMMAND_GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingPhase {
    Welcome,
    Readiness,
    Goals,
    Recommendation,
    Review,
    Applying,
    RollingBack,
    Verifying,
    Result,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EntryPoint {
    FirstRun,
    SoftOffer,
    Settings,
}

impl EntryPoint {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "first_run" => Some(Self::FirstRun),
            "soft_offer" => Some(Self::SoftOffer),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TerminalStatus {
    Active,
    Completed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OfferStatus {
    Pending,
    Accepted,
    Dismissed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingGoals {
    pub dpi: bool,
    pub ai: bool,
    pub telegram: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnboardingDraft {
    pub goals: OnboardingGoals,
    pub dpi_engine: String,
    pub ai_provider: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PlanActionKind {
    ConfigureSettings,
    InstallHosts,
    StartProxy,
    StartDpi,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanAction {
    kind: PlanActionKind,
    title: String,
    detail: String,
    verification: String,
    rollback: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Fingerprints {
    settings: String,
    runtime: String,
    hosts: String,
    operations: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingPlan {
    pub plan_id: String,
    pub draft: OnboardingDraft,
    pub actions: Vec<PlanAction>,
    fingerprints: Fingerprints,
    dpi_config: Option<String>,
    proxy_port: u16,
    fake_tls_domain: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    Inconclusive,
    NotSelected,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationTarget {
    pub id: String,
    pub label: String,
    pub status: VerificationStatus,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Success,
    Partial,
    Failed,
    Inconclusive,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub outcome: VerificationOutcome,
    pub targets: Vec<VerificationTarget>,
    pub accepted: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransactionSnapshot {
    previous_settings: Settings,
    dpi_was_active: bool,
    proxy_was_running: bool,
    hosts_previous_provider: String,
    hosts_previous_installed: bool,
    settings_applied: bool,
    hosts_changed: bool,
    proxy_started: bool,
    dpi_started: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransactionStatus {
    Applying,
    Applied,
    RollingBack,
    RolledBack,
    RecoveryRequired,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingTransaction {
    pub transaction_id: String,
    pub plan_id: String,
    status: TransactionStatus,
    checkpoint: String,
    snapshot: TransactionSnapshot,
    pub verification: Option<VerificationResult>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicOnboardingTransaction {
    pub transaction_id: String,
    pub plan_id: String,
    status: TransactionStatus,
    checkpoint: String,
    pub verification: Option<VerificationResult>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedOnboarding {
    flow_version: u32,
    revision: u64,
    phase: OnboardingPhase,
    entry_point: EntryPoint,
    draft: OnboardingDraft,
    plan: Option<OnboardingPlan>,
    transaction: Option<OnboardingTransaction>,
    verification: Option<VerificationResult>,
    terminal_status: TerminalStatus,
    offer_status: OfferStatus,
    destination: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Presentation {
    Required,
    Offer,
    Modal,
    Hidden,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessSnapshot {
    pub service: bool,
    pub dpi: bool,
    pub hosts: bool,
    pub telegram: bool,
    pub protected_resources: bool,
    pub app_data: bool,
    pub pending_recovery: bool,
    pub proxy_port: bool,
    pub repair_available: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingSnapshot {
    pub flow_version: u32,
    pub revision: u64,
    pub phase: OnboardingPhase,
    presentation: Presentation,
    pub draft: OnboardingDraft,
    pub plan: Option<OnboardingPlan>,
    pub transaction: Option<PublicOnboardingTransaction>,
    pub verification: Option<VerificationResult>,
    pub terminal_status: String,
    pub destination: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnboardingErrorCode {
    Busy,
    InvalidDraft,
    StaleRevision,
    StalePlan,
    RuntimeUnavailable,
    PreflightFailed,
    ApplyFailed,
    RollbackFailed,
    VerificationRequired,
    InvalidTransition,
    RepairUnavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingFailure {
    code: OnboardingErrorCode,
    retryable: bool,
    message_code: &'static str,
    log_path: Option<String>,
}

fn onboarding_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Obsession")
}

fn state_path() -> PathBuf {
    onboarding_dir().join(STATE_FILE)
}

fn log_path() -> PathBuf {
    onboarding_dir().join(LOG_FILE)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn append_log(details: &str) {
    let dir = onboarding_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let sanitized = details.replace(['\r', '\n'], " ");
        let _ = writeln!(file, "{} {}", unix_millis(), sanitized);
    }
}

fn failure(
    code: OnboardingErrorCode,
    retryable: bool,
    message_code: &'static str,
    details: impl AsRef<str>,
) -> OnboardingFailure {
    append_log(details.as_ref());
    OnboardingFailure {
        code,
        retryable,
        message_code,
        log_path: Some(log_path().to_string_lossy().into_owned()),
    }
}

fn invalid_transition(details: impl AsRef<str>) -> OnboardingFailure {
    failure(
        OnboardingErrorCode::InvalidTransition,
        false,
        "onboarding.error.invalid_transition",
        details,
    )
}

fn hash_serializable<T: Serialize>(value: &T) -> Result<String, OnboardingFailure> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.preflight_failed",
            format!("serialize fingerprint: {error}"),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn repair_setup_available() -> bool {
    Path::new(REPAIR_SETUP).is_file()
}

fn port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn app_data_writable() -> bool {
    let dir = onboarding_dir();
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(
        ".write-probe-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(mut file) => {
            let result = file.write_all(b"ok").and_then(|_| file.sync_all());
            drop(file);
            let _ = fs::remove_file(probe);
            result.is_ok()
        }
        Err(_) => false,
    }
}

fn default_draft(settings: &Settings, manual: bool, app: &AppHandle) -> OnboardingDraft {
    let provider = Provider::parse(&settings.ai_provider);
    let hosts_installed = crate::hosts::snapshot_status(app, provider).status == "installed";
    let proxy_running = app.state::<AppState>().proxy.lock_recover().pid.is_some();
    OnboardingDraft {
        goals: OnboardingGoals {
            dpi: if manual {
                !settings.selected_categories.is_empty()
            } else {
                true
            },
            ai: manual && hosts_installed,
            telegram: manual && proxy_running,
        },
        dpi_engine: if manual && settings.dpi_engine == "zapret2" {
            "zapret2".into()
        } else {
            "legacy".into()
        },
        ai_provider: if settings.ai_provider == "geohide" {
            "geohide".into()
        } else {
            "malw".into()
        },
    }
}

fn initial_state(app: &AppHandle) -> PersistedOnboarding {
    let settings = app.state::<AppState>().settings.lock_recover().clone();
    let completed = settings.has_completed_onboarding;
    PersistedOnboarding {
        flow_version: FLOW_VERSION,
        revision: 1,
        phase: OnboardingPhase::Welcome,
        entry_point: EntryPoint::FirstRun,
        draft: default_draft(&settings, false, app),
        plan: None,
        transaction: None,
        verification: None,
        terminal_status: if completed {
            TerminalStatus::Completed
        } else {
            TerminalStatus::Active
        },
        offer_status: OfferStatus::Pending,
        destination: None,
    }
}

fn recover_interrupted_state(mut state: PersistedOnboarding) -> PersistedOnboarding {
    if state.flow_version != FLOW_VERSION {
        state.flow_version = FLOW_VERSION;
        state.revision = state.revision.saturating_add(1);
        state.plan = None;
        state.transaction = None;
        state.verification = None;
        state.phase = OnboardingPhase::Welcome;
    }
    if matches!(
        state.phase,
        OnboardingPhase::Applying | OnboardingPhase::RollingBack
    ) || state.transaction.as_ref().is_some_and(|transaction| {
        matches!(
            transaction.status,
            TransactionStatus::Applying | TransactionStatus::RollingBack
        )
    }) {
        state.phase = OnboardingPhase::RecoveryRequired;
        if let Some(transaction) = state.transaction.as_mut() {
            transaction.status = TransactionStatus::RecoveryRequired;
        }
        state.revision = state.revision.saturating_add(1);
    }
    state
}

fn load_state(app: &AppHandle) -> Result<PersistedOnboarding, OnboardingFailure> {
    let path = state_path();
    if !path.exists() {
        return Ok(initial_state(app));
    }
    let metadata = fs::metadata(&path).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_read",
            format!("onboarding metadata: {error}"),
        )
    })?;
    if metadata.len() > MAX_STATE_BYTES {
        return Err(failure(
            OnboardingErrorCode::PreflightFailed,
            false,
            "onboarding.error.state_invalid",
            format!("onboarding state too large: {}", metadata.len()),
        ));
    }
    let bytes = fs::read(&path).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_read",
            format!("read onboarding state: {error}"),
        )
    })?;
    let document = serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            false,
            "onboarding.error.state_invalid",
            format!("parse onboarding state: {error}"),
        )
    })?;
    let persisted_version = document
        .get("flowVersion")
        .or_else(|| document.get("flow_version"))
        .and_then(serde_json::Value::as_u64);
    if persisted_version != Some(u64::from(FLOW_VERSION)) {
        let archived = path.with_file_name(format!(
            "onboarding.v{}.{}.{}.json",
            persisted_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "unknown".into()),
            unix_millis(),
            std::process::id()
        ));
        fs::rename(&path, &archived).map_err(|error| {
            failure(
                OnboardingErrorCode::PreflightFailed,
                true,
                "onboarding.error.state_archive",
                format!(
                    "archive incompatible onboarding state {} -> {}: {error}",
                    path.display(),
                    archived.display()
                ),
            )
        })?;
        append_log(&format!(
            "archived incompatible onboarding state version {:?} to {}",
            persisted_version,
            archived.display()
        ));
        return Ok(initial_state(app));
    }
    let state = serde_json::from_value(document).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            false,
            "onboarding.error.state_invalid",
            format!("decode onboarding state: {error}"),
        )
    })?;
    Ok(recover_interrupted_state(state))
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let source_w: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_w: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            destination_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

fn persist_state(state: &PersistedOnboarding) -> Result<(), OnboardingFailure> {
    let dir = onboarding_dir();
    fs::create_dir_all(&dir).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_write",
            format!("create onboarding dir: {error}"),
        )
    })?;
    let temporary = dir.join(format!(
        ".onboarding.json.tmp.{}.{}",
        std::process::id(),
        state.revision
    ));
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_write",
            format!("serialize onboarding state: {error}"),
        )
    })?;
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, &state_path())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_write",
            format!("persist onboarding state: {error}"),
        ));
    }
    Ok(())
}

fn presentation(state: &PersistedOnboarding, settings: &Settings) -> Presentation {
    if state.terminal_status == TerminalStatus::Active {
        return if state.entry_point == EntryPoint::FirstRun && !settings.has_completed_onboarding {
            Presentation::Required
        } else {
            Presentation::Modal
        };
    }
    if state.offer_status == OfferStatus::Pending && settings.has_completed_onboarding {
        Presentation::Offer
    } else {
        Presentation::Hidden
    }
}

fn public_transaction(transaction: &OnboardingTransaction) -> PublicOnboardingTransaction {
    PublicOnboardingTransaction {
        transaction_id: transaction.transaction_id.clone(),
        plan_id: transaction.plan_id.clone(),
        status: transaction.status.clone(),
        checkpoint: transaction.checkpoint.clone(),
        verification: transaction.verification.clone(),
    }
}

fn public_snapshot(app: &AppHandle, state: &PersistedOnboarding) -> OnboardingSnapshot {
    let settings = app.state::<AppState>().settings.lock_recover().clone();
    OnboardingSnapshot {
        flow_version: state.flow_version,
        revision: state.revision,
        phase: state.phase,
        presentation: presentation(state, &settings),
        draft: state.draft.clone(),
        plan: state.plan.clone(),
        transaction: state.transaction.as_ref().map(public_transaction),
        verification: state.verification.clone(),
        terminal_status: format!("{:?}", state.terminal_status).to_lowercase(),
        destination: state.destination.clone(),
    }
}

fn check_revision(state: &PersistedOnboarding, expected: u64) -> Result<(), OnboardingFailure> {
    if state.revision == expected {
        Ok(())
    } else {
        Err(failure(
            OnboardingErrorCode::StaleRevision,
            true,
            "onboarding.error.stale_revision",
            format!("expected revision {expected}, actual {}", state.revision),
        ))
    }
}

fn validate_draft(draft: &OnboardingDraft) -> Result<(), OnboardingFailure> {
    if !draft.goals.dpi && !draft.goals.ai && !draft.goals.telegram {
        return Err(failure(
            OnboardingErrorCode::InvalidDraft,
            true,
            "onboarding.error.choose_goal",
            "draft has no selected goals",
        ));
    }
    if !matches!(draft.dpi_engine.as_str(), "legacy" | "zapret2")
        || !matches!(draft.ai_provider.as_str(), "malw" | "geohide")
    {
        return Err(failure(
            OnboardingErrorCode::InvalidDraft,
            false,
            "onboarding.error.invalid_draft",
            "draft contains unsupported enum value",
        ));
    }
    Ok(())
}

fn readiness(app: &AppHandle, state: &PersistedOnboarding) -> ReadinessSnapshot {
    let runtime = crate::protected_runtime::capability_snapshot();
    let app_state = app.state::<AppState>();
    let settings = app_state.settings.lock_recover().clone();
    let proxy_running = app_state.proxy.lock_recover().pid.is_some();
    let protected_resources = app_state.paths.resource_dir().is_dir()
        && !app_state
            .paths
            .get_configs_for_category("discord")
            .is_empty();
    ReadinessSnapshot {
        service: runtime.service_available,
        dpi: runtime.dpi,
        hosts: runtime.hosts,
        telegram: crate::proxy::available(app),
        protected_resources,
        app_data: app_data_writable(),
        pending_recovery: state.phase == OnboardingPhase::RecoveryRequired,
        proxy_port: proxy_running || port_available(settings.proxy_port),
        repair_available: repair_setup_available(),
    }
}

fn current_fingerprints(
    app: &AppHandle,
    draft: &OnboardingDraft,
) -> Result<Fingerprints, OnboardingFailure> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock_recover().clone();
    let runtime = crate::protected_runtime::capability_snapshot();
    let hosts = crate::hosts::snapshot_status(app, Provider::parse(&draft.ai_provider));
    let dpi = crate::protected_runtime::dpi_status();
    let proxy = state.proxy.lock_recover();
    let operations = (
        dpi.active,
        proxy.pid.is_some(),
        settings.proxy_port,
        proxy.pid.is_some() || port_available(settings.proxy_port),
    );
    Ok(Fingerprints {
        settings: hash_serializable(&settings)?,
        runtime: hash_serializable(&runtime)?,
        hosts: hash_serializable(&(
            hosts.provider,
            hosts.status,
            hosts.local_version,
            hosts.rollback_available,
        ))?,
        operations: hash_serializable(&operations)?,
    })
}

fn plan_id(
    draft: &OnboardingDraft,
    fingerprints: &Fingerprints,
) -> Result<String, OnboardingFailure> {
    let digest = hash_serializable(&(FLOW_VERSION, draft, fingerprints))?;
    Ok(format!("plan-{}", &digest[..24]))
}

fn validate_readiness(
    ready: &ReadinessSnapshot,
    draft: &OnboardingDraft,
) -> Result<(), OnboardingFailure> {
    if ready.pending_recovery
        || !ready.service
        || !ready.protected_resources
        || (draft.goals.dpi && !ready.dpi)
        || (draft.goals.ai && !ready.hosts)
        || (draft.goals.telegram && !ready.telegram)
    {
        return Err(failure(
            OnboardingErrorCode::RuntimeUnavailable,
            true,
            "onboarding.error.runtime_unavailable",
            format!("protected runtime readiness rejected plan: {ready:?}"),
        ));
    }
    if !ready.app_data {
        return Err(failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.app_data_unavailable",
            "onboarding state directory is not writable",
        ));
    }
    if draft.goals.telegram && !ready.proxy_port {
        return Err(failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.proxy_port_unavailable",
            "configured Telegram proxy port is already occupied",
        ));
    }
    Ok(())
}

fn build_plan(
    app: &AppHandle,
    draft: &OnboardingDraft,
    state: &PersistedOnboarding,
) -> Result<OnboardingPlan, OnboardingFailure> {
    validate_draft(draft)?;
    let ready = readiness(app, state);
    validate_readiness(&ready, draft)?;
    let settings = app.state::<AppState>().settings.lock_recover().clone();
    let dpi_config = if draft.goals.dpi {
        let configs = app
            .state::<AppState>()
            .paths
            .get_configs_for_category("discord");
        settings
            .selected_configs
            .get("discord")
            .filter(|selected| configs.contains(selected))
            .cloned()
            .or_else(|| configs.first().cloned())
    } else {
        None
    };
    if draft.goals.dpi && dpi_config.is_none() {
        return Err(failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.discord_config_missing",
            "no protected Discord config is available",
        ));
    }
    let mut actions = vec![PlanAction {
        kind: PlanActionKind::ConfigureSettings,
        title: "Сохранить рабочий профиль".into(),
        detail: if draft.goals.dpi {
            format!(
                "Discord станет первой DPI-категорией, движок — {}. Reliability запускается в режиме «Наблюдение».",
                if draft.dpi_engine == "zapret2" { "Zapret2 Beta" } else { "Legacy" }
            )
        } else {
            "Сохраняются только параметры выбранных функций; существующий DPI-профиль не меняется."
                .into()
        },
        verification: "Повторное чтение settings и fingerprints".into(),
        rollback: "Вернуть точный снимок settings".into(),
    }];
    if draft.goals.ai {
        actions.push(PlanAction {
            kind: PlanActionKind::InstallHosts,
            title: "Настроить доступ к ИИ".into(),
            detail: format!("Защищённая служба применит provider {}.", draft.ai_provider),
            verification: "Проверить service-owned hosts и HTTPS к выбранной цели".into(),
            rollback: "Вернуть предыдущий provider или исходный hosts".into(),
        });
    }
    if draft.goals.telegram {
        actions.push(PlanAction {
            kind: PlanActionKind::StartProxy,
            title: "Запустить Telegram-прокси".into(),
            detail: format!(
                "Локальный MTProto endpoint будет запущен на порту {}.",
                settings.proxy_port
            ),
            verification: "Проверить процесс и локальный TCP endpoint".into(),
            rollback: "Остановить только сессию, созданную мастером".into(),
        });
    }
    if draft.goals.dpi {
        actions.push(PlanAction {
            kind: PlanActionKind::StartDpi,
            title: "Включить DPI-обход для Discord".into(),
            detail: format!(
                "Применить защищённую стратегию {}.",
                dpi_config.as_deref().unwrap_or_default()
            ),
            verification: "Проверить runtime и HTTPS к Discord".into(),
            rollback: "Остановить только сессию, созданную мастером".into(),
        });
    }
    let fingerprints = current_fingerprints(app, draft)?;
    Ok(OnboardingPlan {
        plan_id: plan_id(draft, &fingerprints)?,
        draft: draft.clone(),
        actions,
        fingerprints,
        dpi_config,
        proxy_port: settings.proxy_port,
        fake_tls_domain: settings.fake_tls_domain,
    })
}

async fn save_operational_settings(
    app: &AppHandle,
    plan: &OnboardingPlan,
) -> Result<Settings, String> {
    let app_state = app.state::<AppState>();
    let mut next = app_state.settings.lock_recover().clone();
    if plan.draft.goals.dpi {
        next.dpi_engine = plan.draft.dpi_engine.clone();
        let selected_categories = if next.dpi_engine == "zapret2" {
            &mut next.zapret2_selected_categories
        } else {
            &mut next.selected_categories
        };
        if !selected_categories
            .iter()
            .any(|category| category == "discord")
        {
            selected_categories.insert(0, "discord".into());
        }
        if let Some(config) = &plan.dpi_config {
            next.selected_configs
                .insert("discord".into(), config.clone());
        }
        next.legacy_reliability_enabled = true;
        next.legacy_reliability_mode = "observe_only".into();
        next.legacy_automatic_paused = true;
    }
    if plan.draft.goals.ai {
        next.ai_provider = plan.draft.ai_provider.clone();
    }
    let _save = app_state.settings_save_gate.lock().await;
    next.save(&app_state.paths.base_dir)
        .map_err(|error| format!("save operational settings: {error}"))?;
    *app_state.settings.lock_recover() = next.clone();
    app_state.settings_revision.bump();
    crate::protected_runtime::sync_legacy_recovery_controls(&next)?;
    Ok(next)
}

fn collect_dpi_start_pairs<F>(
    settings: &Settings,
    mut configs_for_category: F,
) -> Result<Vec<(String, String)>, String>
where
    F: FnMut(&str) -> Vec<String>,
{
    let selected_categories = if settings.dpi_engine == "zapret2" {
        &settings.zapret2_selected_categories
    } else {
        &settings.selected_categories
    };
    if selected_categories.is_empty() {
        return Err("onboarding DPI profile contains no selected categories".into());
    }

    let mut pairs = Vec::with_capacity(selected_categories.len());
    for category in selected_categories {
        if pairs
            .iter()
            .any(|(known_category, _)| known_category == category)
        {
            return Err(format!(
                "onboarding DPI profile contains duplicate category {category}"
            ));
        }
        let available = configs_for_category(category);
        let config = settings
            .selected_configs
            .get(category)
            .filter(|selected| available.contains(selected))
            .cloned()
            .or_else(|| available.first().cloned())
            .ok_or_else(|| format!("no protected config is available for {category}"))?;
        pairs.push((category.clone(), config));
    }
    Ok(pairs)
}

fn onboarding_dpi_start_pairs(
    app: &AppHandle,
    settings: &Settings,
) -> Result<Vec<(String, String)>, String> {
    let paths = &app.state::<AppState>().paths;
    collect_dpi_start_pairs(settings, |category| {
        paths.get_configs_for_category(category)
    })
}

async fn restore_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _save = state.settings_save_gate.lock().await;
    settings
        .save(&state.paths.base_dir)
        .map_err(|error| format!("restore settings: {error}"))?;
    *state.settings.lock_recover() = settings.clone();
    state.settings_revision.bump();
    crate::protected_runtime::sync_legacy_recovery_controls(settings)?;
    Ok(())
}

fn checkpoint(
    state: &mut PersistedOnboarding,
    status: TransactionStatus,
    value: &str,
) -> Result<(), OnboardingFailure> {
    let transaction = state
        .transaction
        .as_mut()
        .ok_or_else(|| invalid_transition("transaction checkpoint without transaction"))?;
    transaction.status = status;
    transaction.checkpoint = value.into();
    state.revision = state.revision.saturating_add(1);
    persist_state(state)
}

async fn rollback_locked(app: &AppHandle, state: &mut PersistedOnboarding) -> Result<(), String> {
    let snapshot = state
        .transaction
        .as_ref()
        .ok_or_else(|| "rollback transaction missing".to_string())?
        .snapshot
        .clone();
    let mut errors = Vec::new();
    if snapshot.dpi_started {
        if crate::protected_runtime::dpi_status().active {
            if let Err(error) = crate::commands::dpi_stop_locked(app).await {
                errors.push(format!("stop DPI: {error}"));
            }
        }
        if errors.is_empty() {
            state.transaction.as_mut().unwrap().snapshot.dpi_started = false;
            checkpoint(state, TransactionStatus::RollingBack, "dpi_rolled_back")
                .map_err(|error| format!("persist DPI rollback: {:?}", error.code))?;
        }
    }
    if snapshot.proxy_started {
        crate::proxy::stop_locked_async(app).await;
        state.transaction.as_mut().unwrap().snapshot.proxy_started = false;
        checkpoint(state, TransactionStatus::RollingBack, "proxy_rolled_back")
            .map_err(|error| format!("persist proxy rollback: {:?}", error.code))?;
    }
    if snapshot.hosts_changed {
        let result = if snapshot.hosts_previous_installed {
            crate::hosts::install(app, Provider::parse(&snapshot.hosts_previous_provider))
                .await
                .map(|_| ())
        } else {
            crate::hosts::uninstall(app).await
        };
        if let Err(error) = result {
            errors.push(format!("restore hosts: {error}"));
        } else {
            state.transaction.as_mut().unwrap().snapshot.hosts_changed = false;
            checkpoint(state, TransactionStatus::RollingBack, "hosts_rolled_back")
                .map_err(|error| format!("persist hosts rollback: {:?}", error.code))?;
        }
    }
    if snapshot.settings_applied {
        if let Err(error) = restore_settings(app, &snapshot.previous_settings).await {
            errors.push(error);
        } else {
            state
                .transaction
                .as_mut()
                .unwrap()
                .snapshot
                .settings_applied = false;
            checkpoint(
                state,
                TransactionStatus::RollingBack,
                "settings_rolled_back",
            )
            .map_err(|error| format!("persist settings rollback: {:?}", error.code))?;
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[derive(Debug)]
struct HttpsProbe {
    online: bool,
    endpoint: &'static str,
    detail: String,
}

async fn probe_https(url: &'static str) -> HttpsProbe {
    let result = |online: bool, detail: String| HttpsProbe {
        online,
        endpoint: url,
        detail,
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(7))
        .connect_timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        // Protected Legacy lists are IPv4-based. A broken IPv6 route must not
        // turn an otherwise working bypass into a false onboarding failure.
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .user_agent("Obsession/1.1 onboarding-verification")
        .build()
    {
        Ok(client) => client,
        Err(error) => return result(false, format!("client: {error}")),
    };
    match client.get(url).send().await {
        // Любой HTTP response подтверждает DNS + TCP + TLS/SNI. Статусы 403,
        // 429 и даже временный 5xx не означают, что обход не установил канал.
        Ok(response) => result(true, format!("HTTP {}", response.status().as_u16())),
        Err(error) => {
            let stage = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect/TLS"
            } else {
                "request"
            };
            result(false, format!("{stage}: {error}"))
        }
    }
}

async fn probe_any(urls: &'static [&'static str]) -> HttpsProbe {
    let mut tasks = tokio::task::JoinSet::new();
    for &url in urls {
        tasks.spawn(probe_https(url));
    }

    let mut failures = Vec::with_capacity(urls.len());
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(probe) if probe.online => {
                tasks.abort_all();
                return probe;
            }
            Ok(probe) => failures.push(format!("{} {}", probe.endpoint, probe.detail)),
            Err(error) => failures.push(format!("probe task: {error}")),
        }
    }
    HttpsProbe {
        online: false,
        endpoint: "none",
        detail: failures.join(" | "),
    }
}

fn local_proxy_reachable(port: u16) -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(800),
    )
    .is_ok()
}

fn verification_status(
    selected: bool,
    configured: bool,
    reachable: bool,
    control_online: bool,
) -> VerificationStatus {
    if !selected {
        VerificationStatus::NotSelected
    } else if !configured {
        VerificationStatus::Failed
    } else if reachable {
        VerificationStatus::Passed
    } else if control_online {
        VerificationStatus::Failed
    } else {
        VerificationStatus::Inconclusive
    }
}

fn verification_outcome(targets: &[VerificationTarget]) -> VerificationOutcome {
    let selected: Vec<_> = targets
        .iter()
        .filter(|target| target.status != VerificationStatus::NotSelected)
        .collect();
    let passed = selected
        .iter()
        .filter(|target| target.status == VerificationStatus::Passed)
        .count();
    if !selected.is_empty() && passed == selected.len() {
        VerificationOutcome::Success
    } else if passed > 0 {
        VerificationOutcome::Partial
    } else if selected
        .iter()
        .any(|target| target.status == VerificationStatus::Inconclusive)
    {
        VerificationOutcome::Inconclusive
    } else {
        VerificationOutcome::Failed
    }
}

#[tauri::command]
pub async fn onboarding_get_recovery(
    app: AppHandle,
) -> Result<Option<OnboardingSnapshot>, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    // При первом запуске журнал мастера не создаётся и настройки не меняются.
    if !state_path().try_exists().map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.state_read",
            format!("check onboarding state: {error}"),
        )
    })? {
        return Ok(None);
    }
    let state = load_state(&app)?;
    if !needs_recovery_notice(&state) {
        return Ok(None);
    }
    persist_state(&state)?;
    Ok(Some(public_snapshot(&app, &state)))
}

fn needs_recovery_notice(state: &PersistedOnboarding) -> bool {
    state.terminal_status == TerminalStatus::Active && state.transaction.is_some()
}

#[tauri::command]
pub async fn onboarding_get_snapshot(
    app: AppHandle,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let state = load_state(&app)?;
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_start(
    app: AppHandle,
    entry_point: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let entry = EntryPoint::parse(&entry_point).ok_or_else(|| {
        failure(
            OnboardingErrorCode::InvalidDraft,
            false,
            "onboarding.error.entry_point",
            format!("unsupported entry point {entry_point}"),
        )
    })?;
    let mut state = load_state(&app)?;
    let settings = app.state::<AppState>().settings.lock_recover().clone();
    if entry == EntryPoint::FirstRun && settings.has_completed_onboarding {
        return Ok(public_snapshot(&app, &state));
    }
    if state.transaction.is_some() && state.terminal_status == TerminalStatus::Active {
        return Err(failure(
            OnboardingErrorCode::Busy,
            true,
            "onboarding.error.busy",
            "cannot restart onboarding while a transaction is unfinished",
        ));
    }
    state.entry_point = entry;
    state.phase = OnboardingPhase::Welcome;
    state.draft = default_draft(&settings, entry == EntryPoint::Settings, &app);
    state.plan = None;
    state.transaction = None;
    state.verification = None;
    state.terminal_status = TerminalStatus::Active;
    state.destination = None;
    if entry == EntryPoint::SoftOffer {
        state.offer_status = OfferStatus::Accepted;
    }
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_check_readiness(
    app: AppHandle,
) -> Result<ReadinessSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    state.phase = if state.phase == OnboardingPhase::RecoveryRequired {
        OnboardingPhase::RecoveryRequired
    } else {
        OnboardingPhase::Readiness
    };
    state.revision = state.revision.saturating_add(1);
    let snapshot = readiness(&app, &state);
    persist_state(&state)?;
    Ok(snapshot)
}

#[tauri::command]
pub async fn onboarding_save_draft(
    app: AppHandle,
    draft: OnboardingDraft,
    expected_revision: u64,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    validate_draft(&draft)?;
    let mut state = load_state(&app)?;
    check_revision(&state, expected_revision)?;
    if state.transaction.is_some() {
        return Err(invalid_transition(
            "draft cannot change after transaction creation",
        ));
    }
    state.draft = draft;
    state.plan = None;
    state.phase = OnboardingPhase::Recommendation;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_build_plan(
    app: AppHandle,
    draft: OnboardingDraft,
    expected_revision: u64,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    check_revision(&state, expected_revision)?;
    if state.transaction.is_some() {
        return Err(failure(
            OnboardingErrorCode::Busy,
            true,
            "onboarding.error.busy",
            "cannot replace an immutable plan after transaction creation",
        ));
    }
    match build_plan(&app, &draft, &state) {
        Ok(plan) => {
            state.draft = draft;
            state.plan = Some(plan);
            state.phase = OnboardingPhase::Review;
        }
        Err(error) if matches!(error.code, OnboardingErrorCode::RuntimeUnavailable) => {
            state.phase = OnboardingPhase::RecoveryRequired;
            state.revision = state.revision.saturating_add(1);
            persist_state(&state)?;
            return Err(error);
        }
        Err(error) => return Err(error),
    }
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_apply(
    app: AppHandle,
    plan_id: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _command = command_gate().lock().await;
    let app_state = app.state::<AppState>();
    let _dpi = app_state.dpi_gate.lock().await;
    let _proxy = app_state.proxy_gate.lock().await;
    let _hosts = app_state.hosts_gate.lock().await;
    let mut state = load_state(&app)?;
    if let Some(transaction) = &state.transaction {
        if transaction.plan_id == plan_id {
            return Ok(public_snapshot(&app, &state));
        }
        return Err(invalid_transition(
            "another onboarding transaction already exists",
        ));
    }
    let plan = state
        .plan
        .clone()
        .filter(|plan| plan.plan_id == plan_id)
        .ok_or_else(|| {
            failure(
                OnboardingErrorCode::StalePlan,
                true,
                "onboarding.error.stale_plan",
                format!("unknown plan id {plan_id}"),
            )
        })?;
    if current_fingerprints(&app, &plan.draft)? != plan.fingerprints {
        return Err(failure(
            OnboardingErrorCode::StalePlan,
            true,
            "onboarding.error.stale_plan",
            "runtime/settings/hosts/proxy fingerprints changed before apply",
        ));
    }
    let previous_settings = app_state.settings.lock_recover().clone();
    let previous_provider = previous_settings.ai_provider.clone();
    let previous_hosts = crate::hosts::snapshot_status(&app, Provider::parse(&previous_provider));
    let snapshot = TransactionSnapshot {
        previous_settings,
        dpi_was_active: crate::protected_runtime::dpi_status().active,
        proxy_was_running: app_state.proxy.lock_recover().pid.is_some(),
        hosts_previous_provider: previous_provider,
        hosts_previous_installed: previous_hosts.status == "installed",
        settings_applied: false,
        hosts_changed: false,
        proxy_started: false,
        dpi_started: false,
    };
    state.transaction = Some(OnboardingTransaction {
        transaction_id: format!("tx-{}", plan.plan_id.trim_start_matches("plan-")),
        plan_id: plan.plan_id.clone(),
        status: TransactionStatus::Applying,
        checkpoint: "snapshots_saved".into(),
        snapshot,
        verification: None,
    });
    state.phase = OnboardingPhase::Applying;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;

    let apply_result = async {
        state
            .transaction
            .as_mut()
            .unwrap()
            .snapshot
            .settings_applied = true;
        checkpoint(
            &mut state,
            TransactionStatus::Applying,
            "settings_apply_started",
        )
        .map_err(|error| format!("persist settings intent: {:?}", error.code))?;
        let operational_settings = save_operational_settings(&app, &plan).await?;
        checkpoint(&mut state, TransactionStatus::Applying, "settings_applied")
            .map_err(|error| format!("persist settings checkpoint: {:?}", error.code))?;

        if plan.draft.goals.ai {
            let desired = Provider::parse(&plan.draft.ai_provider);
            let status = crate::hosts::snapshot_status(&app, desired);
            if status.status == "external" {
                return Err("hosts was externally modified; refusing onboarding mutation".into());
            }
            if status.status != "installed" {
                state.transaction.as_mut().unwrap().snapshot.hosts_changed = true;
                checkpoint(
                    &mut state,
                    TransactionStatus::Applying,
                    "hosts_apply_started",
                )
                .map_err(|error| format!("persist hosts intent: {:?}", error.code))?;
                crate::hosts::install(&app, desired).await?;
                checkpoint(&mut state, TransactionStatus::Applying, "hosts_applied")
                    .map_err(|error| format!("persist hosts checkpoint: {:?}", error.code))?;
            }
        }

        if plan.draft.goals.telegram
            && !state
                .transaction
                .as_ref()
                .unwrap()
                .snapshot
                .proxy_was_running
        {
            state.transaction.as_mut().unwrap().snapshot.proxy_started = true;
            checkpoint(
                &mut state,
                TransactionStatus::Applying,
                "proxy_start_started",
            )
            .map_err(|error| format!("persist proxy intent: {:?}", error.code))?;
            crate::proxy::start_locked(&app, plan.proxy_port, &plan.fake_tls_domain).await?;
            checkpoint(&mut state, TransactionStatus::Applying, "proxy_started")
                .map_err(|error| format!("persist proxy checkpoint: {:?}", error.code))?;
        }

        if plan.draft.goals.dpi && !state.transaction.as_ref().unwrap().snapshot.dpi_was_active {
            state.transaction.as_mut().unwrap().snapshot.dpi_started = true;
            checkpoint(&mut state, TransactionStatus::Applying, "dpi_start_started")
                .map_err(|error| format!("persist DPI intent: {:?}", error.code))?;
            let pairs = onboarding_dpi_start_pairs(&app, &operational_settings)?;
            crate::util::emit_log(&app, "info", "dpi", "Запрошен запуск DPI: источник=применение начальной настройки");
            crate::commands::dpi_start_locked(&app, pairs).await?;
            checkpoint(&mut state, TransactionStatus::Applying, "dpi_started")
                .map_err(|error| format!("persist dpi checkpoint: {:?}", error.code))?;
        }
        Ok::<(), String>(())
    }
    .await;

    if let Err(error) = apply_result {
        append_log(&format!("apply failed: {error}"));
        state.phase = OnboardingPhase::RollingBack;
        checkpoint(
            &mut state,
            TransactionStatus::RollingBack,
            "apply_failed_rollback",
        )?;
        if let Err(rollback) = rollback_locked(&app, &mut state).await {
            append_log(&format!("apply rollback failed: {rollback}"));
            state.phase = OnboardingPhase::RecoveryRequired;
            checkpoint(
                &mut state,
                TransactionStatus::RecoveryRequired,
                "rollback_incomplete",
            )?;
            return Err(failure(
                OnboardingErrorCode::RollbackFailed,
                true,
                "onboarding.error.rollback_deferred",
                format!("apply={error}; rollback={rollback}"),
            ));
        }
        state.phase = OnboardingPhase::Result;
        checkpoint(
            &mut state,
            TransactionStatus::RolledBack,
            "rollback_complete",
        )?;
        return Err(failure(
            OnboardingErrorCode::ApplyFailed,
            true,
            "onboarding.error.apply_failed",
            error,
        ));
    }

    state.phase = OnboardingPhase::Verifying;
    checkpoint(&mut state, TransactionStatus::Applied, "apply_complete")?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_get_transaction(
    app: AppHandle,
    transaction_id: String,
) -> Result<PublicOnboardingTransaction, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let state = load_state(&app)?;
    state
        .transaction
        .filter(|transaction| transaction.transaction_id == transaction_id)
        .as_ref()
        .map(public_transaction)
        .ok_or_else(|| invalid_transition("transaction id does not match"))
}

#[tauri::command]
pub async fn onboarding_verify(
    app: AppHandle,
    transaction_id: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    let transaction = state
        .transaction
        .as_ref()
        .filter(|transaction| transaction.transaction_id == transaction_id)
        .ok_or_else(|| invalid_transition("verify transaction id does not match"))?;
    if !matches!(transaction.status, TransactionStatus::Applied) {
        return Err(invalid_transition(
            "verification requires an applied transaction",
        ));
    }
    let plan = state
        .plan
        .clone()
        .ok_or_else(|| invalid_transition("verification plan missing"))?;
    state.phase = OnboardingPhase::Verifying;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;

    let (control_probe, discord_probe, ai_health) = tokio::join!(
        probe_any(CONTROL_PROBE_URLS),
        probe_any(DISCORD_PROBE_URLS),
        crate::hosts::check_health(0)
    );
    let dpi_active = crate::protected_runtime::dpi_status().active;
    let hosts = crate::hosts::snapshot_status(&app, Provider::parse(&plan.draft.ai_provider));
    let proxy_running = app.state::<AppState>().proxy.lock_recover().pid.is_some();
    let telegram_reachable = proxy_running && local_proxy_reachable(plan.proxy_port);
    append_log(&format!(
        "verification probes: control={} [{} {}]; discord={} [{} {}]; ai_health={}; dpi_active={dpi_active}; hosts_status={}; proxy_running={proxy_running}; telegram_reachable={telegram_reachable}",
        control_probe.online,
        control_probe.endpoint,
        control_probe.detail,
        discord_probe.online,
        discord_probe.endpoint,
        discord_probe.detail,
        ai_health
            .as_ref()
            .map(|snapshot| format!("{:?}", snapshot.services))
            .unwrap_or_else(|error| format!("unavailable: {error}")),
        hosts.status,
    ));
    let target = |selected: bool,
                  id: &str,
                  label: &str,
                  configured: bool,
                  reachable: bool,
                  control_online: bool,
                  success: &str,
                  not_configured: &str,
                  unreachable: &str,
                  inconclusive: &str| {
        let status = verification_status(selected, configured, reachable, control_online);
        let message = match &status {
            VerificationStatus::NotSelected => "Цель не выбрана",
            VerificationStatus::Passed => success,
            VerificationStatus::Failed if !configured => not_configured,
            VerificationStatus::Failed => unreachable,
            VerificationStatus::Inconclusive => inconclusive,
        };
        VerificationTarget {
            id: id.into(),
            label: label.into(),
            status,
            message: message.into(),
        }
    };
    let ai_target = |service: AiService, id: &str, label: &str| {
        let route = ai_health.as_ref().ok().and_then(|snapshot| {
            snapshot
                .services
                .iter()
                .find(|entry| entry.service == service)
        });
        let configured = hosts.status == "installed"
            && ai_health.as_ref().is_ok_and(|snapshot| snapshot.installed);
        let status = if !plan.draft.goals.ai {
            VerificationStatus::NotSelected
        } else if !configured {
            VerificationStatus::Failed
        } else {
            match route.map(|entry| entry.health) {
                Some(AiRouteHealth::Working) => VerificationStatus::Passed,
                Some(AiRouteHealth::Unavailable) => VerificationStatus::Failed,
                Some(AiRouteHealth::Inconclusive | AiRouteHealth::Unchecked) | None => {
                    VerificationStatus::Inconclusive
                }
            }
        };
        let message = match &status {
            VerificationStatus::NotSelected => "Цель не выбрана".to_owned(),
            VerificationStatus::Passed => {
                let source = route
                    .and_then(|entry| entry.provider)
                    .map(|provider| format!("{provider:?}"))
                    .unwrap_or_else(|| "direct".into());
                let kind = route.map_or(AiRouteKind::Direct, |entry| entry.route);
                format!("HTTPS-маршрут отвечает ({kind:?}, {source})")
            }
            VerificationStatus::Failed if !configured => {
                "Защищённая служба не подтвердила установленный hosts".into()
            }
            VerificationStatus::Failed => {
                "Установленный маршрут не установил HTTPS-соединение".into()
            }
            VerificationStatus::Inconclusive => {
                "Маршрут установлен, но проверка не смогла дать достоверный результат".into()
            }
        };
        VerificationTarget {
            id: id.into(),
            label: label.into(),
            status,
            message,
        }
    };
    let targets = vec![
        target(
            plan.draft.goals.dpi,
            "dpi",
            "Discord / DPI",
            dpi_active,
            discord_probe.online,
            control_probe.online,
            "Runtime активен, Discord отвечает по HTTPS",
            "Защищённый DPI runtime не запущен",
            "Runtime активен, но Discord endpoints не ответили",
            "Runtime активен; автоматическая HTTPS-проверка Discord не получила ответ",
        ),
        ai_target(AiService::Chatgpt, "ai_chatgpt", "ChatGPT"),
        ai_target(AiService::Claude, "ai_claude", "Claude"),
        ai_target(AiService::Gemini, "ai_gemini", "Gemini"),
        target(
            plan.draft.goals.telegram,
            "telegram",
            "Telegram",
            proxy_running,
            telegram_reachable,
            true,
            "Прокси запущен и локальный endpoint принимает соединение",
            "Telegram-прокси не запущен",
            "Локальный endpoint Telegram-прокси не отвечает",
            "Локальный endpoint Telegram-прокси не удалось проверить",
        ),
    ];
    let result = VerificationResult {
        outcome: verification_outcome(&targets),
        targets,
        accepted: false,
    };
    state.verification = Some(result.clone());
    state.transaction.as_mut().unwrap().verification = Some(result);
    state.phase = OnboardingPhase::Result;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_accept_verification(
    app: AppHandle,
    transaction_id: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    let transaction = state
        .transaction
        .as_mut()
        .filter(|transaction| transaction.transaction_id == transaction_id)
        .ok_or_else(|| invalid_transition("accept transaction id does not match"))?;
    let verification = transaction
        .verification
        .as_mut()
        .ok_or_else(|| invalid_transition("verification result missing"))?;
    verification.accepted = true;
    if let Some(result) = state.verification.as_mut() {
        result.accepted = true;
    }
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_rollback(
    app: AppHandle,
    transaction_id: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _command = command_gate().lock().await;
    let app_state = app.state::<AppState>();
    let _dpi = app_state.dpi_gate.lock().await;
    let _proxy = app_state.proxy_gate.lock().await;
    let _hosts = app_state.hosts_gate.lock().await;
    let mut state = load_state(&app)?;
    if state
        .transaction
        .as_ref()
        .is_none_or(|transaction| transaction.transaction_id != transaction_id)
    {
        return Err(invalid_transition("rollback transaction id does not match"));
    }
    state.phase = OnboardingPhase::RollingBack;
    checkpoint(
        &mut state,
        TransactionStatus::RollingBack,
        "rollback_requested",
    )?;
    if let Err(error) = rollback_locked(&app, &mut state).await {
        state.phase = OnboardingPhase::RecoveryRequired;
        checkpoint(
            &mut state,
            TransactionStatus::RecoveryRequired,
            "rollback_incomplete",
        )?;
        return Err(failure(
            OnboardingErrorCode::RollbackFailed,
            true,
            "onboarding.error.rollback_deferred",
            error,
        ));
    }
    state.phase = OnboardingPhase::Result;
    checkpoint(
        &mut state,
        TransactionStatus::RolledBack,
        "rollback_complete",
    )?;
    Ok(public_snapshot(&app, &state))
}

async fn set_shadow_completed(app: &AppHandle, completed: bool) -> Result<(), OnboardingFailure> {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock_recover().clone();
    settings.has_completed_onboarding = completed;
    let _save = state.settings_save_gate.lock().await;
    settings.save(&state.paths.base_dir).map_err(|error| {
        failure(
            OnboardingErrorCode::PreflightFailed,
            true,
            "onboarding.error.settings_write",
            format!("save compatibility shadow: {error}"),
        )
    })?;
    *state.settings.lock_recover() = settings;
    state.settings_revision.bump();
    Ok(())
}

#[tauri::command]
pub async fn onboarding_complete(
    app: AppHandle,
    transaction_id: String,
    destination: String,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    if !matches!(destination.as_str(), "overview" | "dpi" | "ai" | "telegram") {
        return Err(invalid_transition("unsupported onboarding destination"));
    }
    let mut state = load_state(&app)?;
    let transaction = state
        .transaction
        .as_ref()
        .filter(|transaction| transaction.transaction_id == transaction_id)
        .ok_or_else(|| invalid_transition("complete transaction id does not match"))?;
    let accepted = transaction
        .verification
        .as_ref()
        .is_some_and(|verification| {
            verification.outcome == VerificationOutcome::Success || verification.accepted
        })
        || transaction.status == TransactionStatus::RolledBack;
    if !accepted {
        return Err(failure(
            OnboardingErrorCode::VerificationRequired,
            true,
            "onboarding.error.verification_choice_required",
            "completion attempted before verification choice",
        ));
    }
    set_shadow_completed(&app, true).await?;
    state.terminal_status = TerminalStatus::Completed;
    state.offer_status = OfferStatus::Accepted;
    state.destination = Some(destination);
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_skip(
    app: AppHandle,
    expected_revision: u64,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    check_revision(&state, expected_revision)?;
    if state.transaction.is_some() {
        return Err(invalid_transition("cannot skip after apply"));
    }
    if state.terminal_status == TerminalStatus::Active {
        set_shadow_completed(&app, true).await?;
        state.terminal_status = TerminalStatus::Skipped;
    }
    state.offer_status = OfferStatus::Dismissed;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn onboarding_cancel(
    app: AppHandle,
    expected_revision: u64,
) -> Result<OnboardingSnapshot, OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let mut state = load_state(&app)?;
    check_revision(&state, expected_revision)?;
    if state.transaction.is_some() {
        return Err(invalid_transition("cannot cancel after apply"));
    }
    state.terminal_status = TerminalStatus::Cancelled;
    state.revision = state.revision.saturating_add(1);
    persist_state(&state)?;
    Ok(public_snapshot(&app, &state))
}

#[tauri::command]
pub async fn launch_repair_setup(app: AppHandle) -> Result<(), OnboardingFailure> {
    let _gate = command_gate().lock().await;
    let requested = Path::new(REPAIR_SETUP);
    let canonical = fs::canonicalize(requested).map_err(|error| {
        failure(
            OnboardingErrorCode::RepairUnavailable,
            true,
            "onboarding.error.repair_unavailable",
            format!("canonicalize fixed repair setup: {error}"),
        )
    })?;
    let expected = fs::canonicalize(Path::new(r"C:\Program Files\Obsession"))
        .map_err(|error| {
            failure(
                OnboardingErrorCode::RepairUnavailable,
                true,
                "onboarding.error.repair_unavailable",
                format!("canonicalize Program Files product directory: {error}"),
            )
        })?
        .join("uninstall.exe");
    if canonical.to_string_lossy().to_lowercase() != expected.to_string_lossy().to_lowercase()
        || !canonical.is_file()
    {
        return Err(failure(
            OnboardingErrorCode::RepairUnavailable,
            false,
            "onboarding.error.repair_unavailable",
            format!(
                "fixed repair setup identity mismatch: {}",
                canonical.display()
            ),
        ));
    }
    Command::new(&canonical).spawn().map_err(|error| {
        failure(
            OnboardingErrorCode::RepairUnavailable,
            true,
            "onboarding.error.repair_launch",
            format!("launch fixed repair setup: {error}"),
        )
    })?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_notice_preserves_interrupted_work_and_ignores_welcome() {
        let mut state = PersistedOnboarding {
            flow_version: FLOW_VERSION,
            revision: 1,
            phase: OnboardingPhase::Welcome,
            entry_point: EntryPoint::FirstRun,
            draft: draft(),
            plan: None,
            transaction: None,
            verification: None,
            terminal_status: TerminalStatus::Active,
            offer_status: OfferStatus::Pending,
            destination: None,
        };
        assert!(!needs_recovery_notice(&state));
        state.phase = OnboardingPhase::Review;
        assert!(!needs_recovery_notice(&state));
        state.transaction = Some(OnboardingTransaction {
            transaction_id: "old-transaction".into(),
            plan_id: "old-plan".into(),
            status: TransactionStatus::Applying,
            checkpoint: "hosts_changed".into(),
            snapshot: TransactionSnapshot {
                previous_settings: Settings::default(),
                dpi_was_active: false,
                proxy_was_running: false,
                hosts_previous_provider: "malw".into(),
                hosts_previous_installed: false,
                settings_applied: true,
                hosts_changed: true,
                proxy_started: false,
                dpi_started: false,
            },
            verification: None,
        });
        state.phase = OnboardingPhase::Applying;
        let mut restored = recover_interrupted_state(state);
        assert!(needs_recovery_notice(&restored));
        assert_eq!(restored.phase, OnboardingPhase::RecoveryRequired);
        let transaction = restored.transaction.as_ref().unwrap();
        assert_eq!(transaction.transaction_id, "old-transaction");
        assert_eq!(transaction.checkpoint, "hosts_changed");
        assert!(transaction.snapshot.hosts_changed);
        for status in [TransactionStatus::Applied, TransactionStatus::RolledBack] {
            restored.transaction.as_mut().unwrap().status = status;
            assert!(needs_recovery_notice(&restored));
        }
        restored.terminal_status = TerminalStatus::Completed;
        assert!(!needs_recovery_notice(&restored));
    }

    fn draft() -> OnboardingDraft {
        OnboardingDraft {
            goals: OnboardingGoals {
                dpi: true,
                ai: true,
                telegram: false,
            },
            dpi_engine: "legacy".into(),
            ai_provider: "malw".into(),
        }
    }

    fn fingerprints() -> Fingerprints {
        Fingerprints {
            settings: "a".into(),
            runtime: "b".into(),
            hosts: "c".into(),
            operations: "d".into(),
        }
    }

    #[test]
    fn canonical_plan_id_is_deterministic_and_sensitive_to_draft() {
        let first = plan_id(&draft(), &fingerprints()).unwrap();
        let second = plan_id(&draft(), &fingerprints()).unwrap();
        assert_eq!(first, second);
        let mut changed = draft();
        changed.goals.telegram = true;
        assert_ne!(first, plan_id(&changed, &fingerprints()).unwrap());
    }

    #[test]
    fn invalid_and_empty_drafts_are_rejected() {
        let mut value = draft();
        value.goals = OnboardingGoals {
            dpi: false,
            ai: false,
            telegram: false,
        };
        assert!(validate_draft(&value).is_err());
        value.goals.dpi = true;
        value.dpi_engine = "unknown".into();
        assert!(validate_draft(&value).is_err());
    }

    #[test]
    fn verification_distinguishes_success_partial_failure_and_inconclusive() {
        let passed = VerificationTarget {
            id: "dpi".into(),
            label: "DPI".into(),
            status: VerificationStatus::Passed,
            message: String::new(),
        };
        let failed = VerificationTarget {
            status: VerificationStatus::Failed,
            ..passed.clone()
        };
        let inconclusive = VerificationTarget {
            status: VerificationStatus::Inconclusive,
            ..passed.clone()
        };
        assert_eq!(
            verification_outcome(&[passed.clone()]),
            VerificationOutcome::Success
        );
        assert_eq!(
            verification_outcome(&[passed, failed.clone()]),
            VerificationOutcome::Partial
        );
        assert_eq!(verification_outcome(&[failed]), VerificationOutcome::Failed);
        assert_eq!(
            verification_outcome(&[inconclusive]),
            VerificationOutcome::Inconclusive
        );
    }

    #[test]
    fn verification_status_separates_configuration_failures_from_network_uncertainty() {
        assert_eq!(
            verification_status(false, false, false, false),
            VerificationStatus::NotSelected
        );
        assert_eq!(
            verification_status(true, false, false, false),
            VerificationStatus::Failed
        );
        assert_eq!(
            verification_status(true, true, true, false),
            VerificationStatus::Passed
        );
        assert_eq!(
            verification_status(true, true, false, true),
            VerificationStatus::Failed
        );
        assert_eq!(
            verification_status(true, true, false, false),
            VerificationStatus::Inconclusive
        );
    }

    #[test]
    fn transaction_snapshot_round_trips_but_stays_out_of_public_payload() {
        let transaction = OnboardingTransaction {
            transaction_id: "tx-test".into(),
            plan_id: "plan-test".into(),
            status: TransactionStatus::Applying,
            checkpoint: "proxy_started".into(),
            snapshot: TransactionSnapshot {
                previous_settings: Settings::default(),
                dpi_was_active: false,
                proxy_was_running: false,
                hosts_previous_provider: "malw".into(),
                hosts_previous_installed: false,
                settings_applied: true,
                hosts_changed: true,
                proxy_started: true,
                dpi_started: false,
            },
            verification: None,
        };
        let state = PersistedOnboarding {
            flow_version: FLOW_VERSION,
            revision: 7,
            phase: OnboardingPhase::Applying,
            entry_point: EntryPoint::FirstRun,
            draft: draft(),
            plan: None,
            transaction: Some(transaction.clone()),
            verification: None,
            terminal_status: TerminalStatus::Active,
            offer_status: OfferStatus::Pending,
            destination: None,
        };

        let disk_json = serde_json::to_value(&state).unwrap();
        assert!(disk_json["transaction"]["snapshot"].is_object());
        let restored: PersistedOnboarding = serde_json::from_value(disk_json).unwrap();
        assert!(restored.transaction.unwrap().snapshot.proxy_started);

        let public_json = serde_json::to_value(public_transaction(&transaction)).unwrap();
        assert!(public_json.get("snapshot").is_none());
        assert_eq!(public_json["checkpoint"], "proxy_started");
    }

    #[test]
    fn readiness_distinguishes_runtime_repair_from_local_preflight_failures() {
        let mut value = draft();
        value.goals.telegram = true;
        let mut ready = ReadinessSnapshot {
            service: true,
            dpi: true,
            hosts: true,
            telegram: true,
            protected_resources: true,
            app_data: true,
            pending_recovery: false,
            proxy_port: true,
            repair_available: true,
        };
        assert!(validate_readiness(&ready, &value).is_ok());

        ready.proxy_port = false;
        assert!(matches!(
            validate_readiness(&ready, &value).unwrap_err().code,
            OnboardingErrorCode::PreflightFailed
        ));

        ready.proxy_port = true;
        ready.pending_recovery = true;
        assert!(matches!(
            validate_readiness(&ready, &value).unwrap_err().code,
            OnboardingErrorCode::RuntimeUnavailable
        ));
    }

    #[test]
    fn onboarding_dpi_start_preserves_every_selected_category() {
        let mut settings = Settings::default();
        settings.dpi_engine = "legacy".into();
        settings.selected_categories = vec!["youtube_twitch".into(), "discord".into()];
        settings
            .selected_configs
            .insert("youtube_twitch".into(), "youtube_twitch_4.conf".into());
        settings
            .selected_configs
            .insert("discord".into(), "discord_9.conf".into());

        let pairs = collect_dpi_start_pairs(&settings, |category| match category {
            "youtube_twitch" => vec![
                "youtube_twitch_1.conf".into(),
                "youtube_twitch_4.conf".into(),
            ],
            "discord" => vec!["discord_1.conf".into(), "discord_9.conf".into()],
            _ => Vec::new(),
        })
        .unwrap();

        assert_eq!(
            pairs,
            [
                ("youtube_twitch".into(), "youtube_twitch_4.conf".into()),
                ("discord".into(), "discord_9.conf".into()),
            ]
        );
    }

    #[test]
    fn onboarding_dpi_start_uses_the_selected_engine_category_set() {
        let mut settings = Settings::default();
        settings.dpi_engine = "zapret2".into();
        settings.selected_categories = vec!["universal".into()];
        settings.zapret2_selected_categories = vec!["discord".into(), "youtube_twitch".into()];

        let pairs =
            collect_dpi_start_pairs(&settings, |category| vec![format!("{category}_1.conf")])
                .unwrap();

        assert_eq!(
            pairs,
            [
                ("discord".into(), "discord_1.conf".into()),
                ("youtube_twitch".into(), "youtube_twitch_1.conf".into()),
            ]
        );
    }
}
