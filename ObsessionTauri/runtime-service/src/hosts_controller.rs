//! Protected, service-owned controller for the Windows system hosts file.
//!
//! IPC selects only an allowlisted provider. System paths, download URLs,
//! backup names and file contents are reconstructed inside LocalSystem.

#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::windows::ffi::OsStrExt;
#[cfg(not(test))]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use obsession_runtime_protocol::{
    AiRouteFailureReason, AiRouteHealth, AiRouteKind, AiService, AiServiceRouteHealth,
    HostsHealthSnapshot, HostsMutationRequest, HostsProvider, HostsRuntimeSnapshot,
    OperationAccepted,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
#[cfg(not(test))]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::dpi_materializer::{ProtectedDataLayout, RUNTIME_STATE_RELATIVE};
use crate::BackendError;

const STATE_SCHEMA_VERSION: u32 = 2;
const MAX_HOSTS_BYTES: usize = 10 * 1024 * 1024;
const MAX_VERSION_BYTES: usize = 128;
const MARKER_PREFIX: &str = "# obsession:ai-provider=";
const USER_BLOCK_BEGIN: &str = "# obsession:user-preserved-begin";
const USER_BLOCK_END: &str = "# obsession:user-preserved-end";
const HOSTS_STATE_DIRECTORY: &str = "hosts";
const HOSTS_STATE_FILE: &str = "hosts-state.json";
const HOSTS_BACKUPS_DIRECTORY: &str = "backups";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_BUDGET: Duration = Duration::from_secs(25);
const MAX_PROBE_CANDIDATES_PER_DOMAIN: usize = 4;
const MAX_PARALLEL_PROBES: usize = 6;
const POST_WRITE_DNS_SETTLE_DELAY: Duration = Duration::from_secs(20);
const POST_WRITE_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(8)];
const ATOMIC_WRITE_RETRY_DELAYS_MS: [u64; 3] = [40, 120, 360];
#[cfg(not(test))]
const CREATE_NO_WINDOW_FLAG: u32 = CREATE_NO_WINDOW.0;

static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

trait HostsDownloader: Send + Sync {
    fn download(&self, provider: HostsProvider) -> Result<Vec<u8>, BackendError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProbeOutcome {
    working: bool,
    reason: Option<AiRouteFailureReason>,
    elapsed_millis: u64,
}

trait HostsRouteProber: Send + Sync {
    fn probe(&self, host: &str, path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome;
}

struct HttpHostsDownloader {
    client: reqwest::blocking::Client,
}

impl HttpHostsDownloader {
    fn new() -> Result<Self, BackendError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .user_agent("Obsession-Runtime/1.1")
            .build()
            .map_err(|_| BackendError::ServiceUnavailable)?;
        Ok(Self { client })
    }
}

impl HostsDownloader for HttpHostsDownloader {
    fn download(&self, provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
        let response = self
            .client
            .get(provider_url(provider))
            .send()
            .map_err(|_| BackendError::RuntimeFailed)?
            .error_for_status()
            .map_err(|_| BackendError::RuntimeFailed)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_HOSTS_BYTES as u64)
        {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_HOSTS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BackendError::RuntimeFailed)?;
        if bytes.len() > MAX_HOSTS_BYTES {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        Ok(bytes)
    }
}

struct HttpsRouteProber;

impl HostsRouteProber for HttpsRouteProber {
    fn probe(&self, host: &str, path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome {
        let started = Instant::now();
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .connect_timeout(PROBE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent("Obsession-Runtime/1.1");
        if let Some(address) = address {
            builder = builder.resolve(host, SocketAddr::from((address, 443)));
        }
        let client = match builder.build() {
            Ok(client) => client,
            Err(_) => {
                return ProbeOutcome {
                    working: false,
                    reason: Some(AiRouteFailureReason::Tls),
                    elapsed_millis: elapsed_millis(started),
                };
            }
        };
        let url = format!("https://{host}{path}");
        let mut last_reason = AiRouteFailureReason::Timeout;
        for _ in 0..2 {
            match client.get(&url).send() {
                // Any valid HTTP response proves that TCP, TLS/SNI and the
                // selected route work. Reading a response body made slow or
                // intentionally streaming endpoints produce false timeouts.
                Ok(_) => {
                    return ProbeOutcome {
                        working: true,
                        reason: None,
                        elapsed_millis: elapsed_millis(started),
                    };
                }
                Err(error) => {
                    last_reason = if error.is_timeout() {
                        AiRouteFailureReason::Timeout
                    } else if error.is_connect() {
                        AiRouteFailureReason::Tls
                    } else {
                        AiRouteFailureReason::Dns
                    };
                }
            }
        }
        ProbeOutcome {
            working: false,
            reason: Some(last_reason),
            elapsed_millis: elapsed_millis(started),
        }
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotRef {
    file: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ProviderState {
    #[serde(default)]
    applied_sha256: Option<String>,
    #[serde(default)]
    applied_payload: Option<SnapshotRef>,
    #[serde(default)]
    verified_last_known_good: Option<SnapshotRef>,
    #[serde(default)]
    verified_health: Option<HostsHealthSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HostsManagedState {
    schema_version: u32,
    original: Option<SnapshotRef>,
    providers: BTreeMap<String, ProviderState>,
    active_provider: Option<HostsProvider>,
    preferred_provider: HostsProvider,
    #[serde(default)]
    health: Option<HostsHealthSnapshot>,
    /// A pre-operation snapshot persisted before every system-file write.
    /// Startup restores it when the service died between write and commit.
    pending_rollback: Option<SnapshotRef>,
}

impl Default for HostsManagedState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            original: None,
            providers: BTreeMap::new(),
            active_provider: None,
            preferred_provider: HostsProvider::Malw,
            health: None,
            pending_rollback: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LegacyProviderState {
    applied_sha256: Option<String>,
    last_known_good: Option<SnapshotRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LegacyHostsManagedState {
    schema_version: u32,
    original: Option<SnapshotRef>,
    providers: BTreeMap<String, LegacyProviderState>,
    active_provider: Option<HostsProvider>,
    pending_rollback: Option<SnapshotRef>,
}

pub(crate) struct HostsController {
    hosts_path: PathBuf,
    state_path: PathBuf,
    backups_dir: PathBuf,
    state: HostsManagedState,
    downloader: Arc<dyn HostsDownloader>,
    prober: Arc<dyn HostsRouteProber>,
    next_operation_id: u64,
}

#[derive(Clone, Copy)]
struct ProbeTarget {
    host: &'static str,
    path: &'static str,
}

const CONTROL_TARGETS: [ProbeTarget; 2] = [
    ProbeTarget {
        host: "www.gstatic.com",
        path: "/generate_204",
    },
    ProbeTarget {
        host: "www.msftconnecttest.com",
        path: "/connecttest.txt",
    },
];
const CHATGPT_TARGETS: [ProbeTarget; 3] = [
    ProbeTarget {
        host: "chatgpt.com",
        path: "/",
    },
    ProbeTarget {
        host: "auth.openai.com",
        path: "/",
    },
    ProbeTarget {
        host: "cdn.oaistatic.com",
        path: "/",
    },
];
const CLAUDE_TARGETS: [ProbeTarget; 2] = [
    ProbeTarget {
        host: "claude.ai",
        path: "/",
    },
    ProbeTarget {
        host: "api.anthropic.com",
        path: "/",
    },
];
const GEMINI_TARGETS: [ProbeTarget; 3] = [
    ProbeTarget {
        host: "gemini.google.com",
        path: "/",
    },
    ProbeTarget {
        host: "aistudio.google.com",
        path: "/",
    },
    ProbeTarget {
        host: "generativelanguage.googleapis.com",
        path: "/",
    },
];

#[derive(Clone, Debug)]
struct PreparedFeed {
    provider: HostsProvider,
    normalized: String,
    ipv4: BTreeMap<String, Vec<Ipv4Addr>>,
}

#[derive(Clone, Debug)]
struct ServiceRoutePlan {
    service: AiService,
    route: AiRouteKind,
    provider: Option<HostsProvider>,
    selected_candidates: BTreeMap<String, Ipv4Addr>,
}

fn all_services() -> [AiService; 3] {
    [AiService::Chatgpt, AiService::Claude, AiService::Gemini]
}

fn probe_targets(service: AiService) -> &'static [ProbeTarget] {
    match service {
        AiService::Chatgpt => &CHATGPT_TARGETS,
        AiService::Claude => &CLAUDE_TARGETS,
        AiService::Gemini => &GEMINI_TARGETS,
    }
}

fn service_for_domain(domain: &str) -> Option<AiService> {
    let domain = domain.to_ascii_lowercase();
    let suffix = |value: &str| domain == value || domain.ends_with(&format!(".{value}"));
    if suffix("chatgpt.com")
        || suffix("openai.com")
        || suffix("oaistatic.com")
        || suffix("oaiusercontent.com")
    {
        return Some(AiService::Chatgpt);
    }
    if suffix("claude.ai") || suffix("claude.com") || suffix("anthropic.com") {
        return Some(AiService::Claude);
    }
    if matches!(
        domain.as_str(),
        "gemini.google.com"
            | "gemini.google"
            | "bard.google.com"
            | "aistudio.google.com"
            | "generativelanguage.googleapis.com"
            | "aisandbox-pa.googleapis.com"
            | "robinfrontend-pa.googleapis.com"
            | "alkalimakersuite-pa.clients6.google.com"
            | "webchannel-alkalimakersuite-pa.clients6.google.com"
    ) {
        return Some(AiService::Gemini);
    }
    None
}

fn other_provider(provider: HostsProvider) -> HostsProvider {
    match provider {
        HostsProvider::Malw => HostsProvider::Geohide,
        HostsProvider::Geohide => HostsProvider::Malw,
    }
}

impl HostsController {
    pub(crate) fn discover(layout: &ProtectedDataLayout) -> Result<Self, BackendError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .ok_or(BackendError::ServiceUnavailable)?;
        let canonical_program_data =
            fs::canonicalize(program_data).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_layout =
            fs::canonicalize(layout.root()).map_err(|_| BackendError::ServiceUnavailable)?;
        if !path_eq(
            &canonical_layout,
            &canonical_program_data.join(RUNTIME_STATE_RELATIVE),
        ) {
            return Err(BackendError::ServiceUnavailable);
        }
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or(BackendError::ServiceUnavailable)?;
        if !system_root.is_absolute() {
            return Err(BackendError::ServiceUnavailable);
        }
        let hosts_path = system_root.join("System32/drivers/etc/hosts");
        if !hosts_path.is_file() {
            return Err(BackendError::ServiceUnavailable);
        }
        let state_root = layout.root().join(HOSTS_STATE_DIRECTORY);
        fs::create_dir_all(&state_root).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_parent =
            fs::canonicalize(layout.root()).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_state =
            fs::canonicalize(&state_root).map_err(|_| BackendError::ServiceUnavailable)?;
        let expected = canonical_parent.join(HOSTS_STATE_DIRECTORY);
        if !path_eq(&canonical_state, &expected) {
            return Err(BackendError::ServiceUnavailable);
        }
        let backups_dir = state_root.join(HOSTS_BACKUPS_DIRECTORY);
        fs::create_dir_all(&backups_dir).map_err(|_| BackendError::ServiceUnavailable)?;
        Self::from_paths(
            hosts_path,
            state_root.join(HOSTS_STATE_FILE),
            backups_dir,
            Arc::new(HttpHostsDownloader::new()?),
            Arc::new(HttpsRouteProber),
        )
    }

    fn from_paths(
        hosts_path: PathBuf,
        state_path: PathBuf,
        backups_dir: PathBuf,
        downloader: Arc<dyn HostsDownloader>,
        prober: Arc<dyn HostsRouteProber>,
    ) -> Result<Self, BackendError> {
        fs::create_dir_all(&backups_dir).map_err(|_| BackendError::ServiceUnavailable)?;
        let mut controller = Self {
            state: load_state(&state_path),
            hosts_path,
            state_path,
            backups_dir,
            downloader,
            prober,
            next_operation_id: 0,
        };
        controller.recover_interrupted_transaction()?;
        Ok(controller)
    }

    pub(crate) fn snapshot(&self) -> Option<HostsRuntimeSnapshot> {
        let current = fs::read(&self.hosts_path).ok()?;
        let provider = self
            .state
            .active_provider
            .or_else(|| detect_managed_provider(&current))?;
        let provider_state = self.state.providers.get(provider_key(provider));
        let externally_modified = provider_state
            .and_then(|state| state.applied_sha256.as_deref())
            .is_some_and(|expected| sha256_hex(&current) != expected);
        Some(HostsRuntimeSnapshot {
            provider,
            installed: contains_marker(&current, provider),
            externally_modified,
            rollback_available: provider_state
                .and_then(|state| state.verified_last_known_good.as_ref())
                .is_some(),
            local_version: extract_version(&current),
        })
    }

    pub(crate) fn check(
        &mut self,
        max_age_seconds: u32,
    ) -> Result<HostsHealthSnapshot, BackendError> {
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        let provider = self
            .state
            .active_provider
            .or_else(|| detect_managed_provider(&current));
        let preferred = provider.unwrap_or(self.state.preferred_provider);
        let installed = provider.is_some_and(|value| contains_marker(&current, value));
        let externally_modified = provider
            .and_then(|value| self.state.providers.get(provider_key(value)))
            .and_then(|state| state.applied_sha256.as_deref())
            .is_some_and(|expected| sha256_hex(&current) != expected);
        let plans = plans_from_cached_health(self.state.health.as_ref(), preferred);

        if !externally_modified && max_age_seconds > 0 {
            if let Some(cached) = self.state.health.as_ref().filter(|snapshot| {
                snapshot.installed == installed
                    && snapshot.checked_at_unix.is_some_and(|checked| {
                        unix_now().saturating_sub(checked) <= u64::from(max_age_seconds)
                    })
            }) {
                return Ok(cached.clone());
            }
        }

        let health = if externally_modified {
            uniform_health_for_plans(
                preferred,
                installed,
                &plans,
                AiRouteHealth::Inconclusive,
                Some(AiRouteFailureReason::ExternalChange),
                true,
            )
        } else if !installed {
            unchecked_health(preferred, false)
        } else if !controls_available(self.prober.clone()) {
            uniform_health_for_plans(
                preferred,
                true,
                &plans,
                AiRouteHealth::Inconclusive,
                Some(AiRouteFailureReason::Offline),
                false,
            )
        } else {
            stabilize_route_health(
                self.state.health.as_ref(),
                check_system_routes(self.prober.clone(), preferred, &plans),
            )
        };
        self.state.preferred_provider = preferred;
        self.state.health = Some(health.clone());
        self.save_state()?;
        Ok(health)
    }

    pub(crate) fn install(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        let provider = request.provider;
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if self.state.original.is_none() {
            if detect_managed_provider(&current).is_some() {
                // An older per-user installation has no protected original.
                // Adopting it would make safe uninstall impossible.
                return Err(BackendError::Conflict);
            }
            let original = self.write_snapshot("original", &current)?;
            self.state.original = Some(original);
            self.save_state()?;
        }
        let previous_managed = self.active_managed_snapshot()?;
        if !controls_available(self.prober.clone()) {
            return Err(BackendError::RuntimeFailed);
        }

        let preferred_feed = prepare_payload(provider, &self.downloader.download(provider)?)?;
        let fallback_provider = other_provider(provider);
        let fallback_feed = prepare_payload(
            fallback_provider,
            &self.downloader.download(fallback_provider)?,
        )?;
        let plans = plan_service_routes(
            self.prober.clone(),
            provider,
            &preferred_feed,
            &fallback_feed,
        );
        let managed_payload = render_hybrid_payload(&preferred_feed, &fallback_feed, &plans)?;
        let prepared = merge_preserving_user_entries(
            &managed_payload,
            &current,
            previous_managed.as_deref(),
            true,
        )?;

        let before_commit = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if sha256_hex(&before_commit) != sha256_hex(&current) {
            return Err(BackendError::Conflict);
        }

        let operation_id = self.begin_transaction(&before_commit)?;
        let transaction_snapshot = self.pending_backup().clone();
        if let Err(error) = atomic_write(&self.hosts_path, &prepared) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let readback = fs::read(&self.hosts_path).map_err(map_io)?;
        if sha256_hex(&readback) != sha256_hex(&prepared) {
            let _ = self.rollback_pending();
            return Err(BackendError::RuntimeFailed);
        }
        flush_dns_best_effort();
        let health = verify_post_write_routes(self.prober.clone(), provider, &plans);
        if !post_write_health_is_valid(&plans, &health) {
            let _ = self.rollback_pending();
            flush_dns_best_effort();
            return Err(BackendError::RuntimeFailed);
        }

        let applied =
            match self.write_snapshot(&format!("applied-{operation_id}"), &managed_payload) {
                Ok(applied) => applied,
                Err(error) => {
                    let _ = self.rollback_pending();
                    return Err(error);
                }
            };
        let all_working = health
            .services
            .iter()
            .all(|service| service.health == AiRouteHealth::Working);
        let verified = if all_working {
            match self.write_snapshot(&format!("lkg-{operation_id}"), &managed_payload) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    let _ = self.rollback_pending();
                    remove_snapshot(&self.backups_dir, &applied);
                    return Err(error);
                }
            }
        } else {
            None
        };
        let state_before_commit = self.state.clone();
        let previous_applied = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.applied_payload.clone());
        let previous_verified = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.verified_last_known_good.clone());
        let provider_state = self
            .state
            .providers
            .entry(provider_key(provider).to_owned())
            .or_default();
        provider_state.applied_sha256 = Some(sha256_hex(&prepared));
        provider_state.applied_payload = Some(applied.clone());
        if let Some(verified) = verified.as_ref() {
            provider_state.verified_last_known_good = Some(verified.clone());
            provider_state.verified_health = Some(health.clone());
        }
        self.state.active_provider = Some(provider);
        self.state.preferred_provider = provider;
        self.state.health = Some(health);
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            remove_snapshot(&self.backups_dir, &applied);
            if let Some(verified) = verified.as_ref() {
                remove_snapshot(&self.backups_dir, verified);
            }
            return Err(error);
        }
        if let Some(previous) = previous_applied.filter(|old| old.file != applied.file) {
            remove_snapshot(&self.backups_dir, &previous);
        }
        if let (Some(previous), Some(verified)) = (previous_verified, verified.as_ref()) {
            if previous.file != verified.file {
                remove_snapshot(&self.backups_dir, &previous);
            }
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        Ok(OperationAccepted { operation_id })
    }

    pub(crate) fn uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        let previous_managed = self.active_managed_snapshot()?;
        let original = self
            .state
            .original
            .clone()
            .or_else(|| find_orphaned_original(&self.backups_dir))
            .ok_or(BackendError::Conflict)?;
        let original_bytes = read_snapshot(&self.backups_dir, &original).map_err(map_io)?;
        let prepared = merge_preserving_user_entries(
            &original_bytes,
            &current,
            previous_managed.as_deref(),
            false,
        )?;
        let before_commit = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if sha256_hex(&before_commit) != sha256_hex(&current) {
            return Err(BackendError::Conflict);
        }
        let operation_id = self.begin_transaction(&current)?;
        let transaction_snapshot = self.pending_backup().clone();
        if let Err(error) = atomic_write(&self.hosts_path, &prepared) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let readback = fs::read(&self.hosts_path).map_err(map_io)?;
        if sha256_hex(&readback) != sha256_hex(&prepared) {
            let _ = self.rollback_pending();
            return Err(BackendError::RuntimeFailed);
        }
        let state_before_commit = self.state.clone();
        let active_provider = self.state.active_provider;
        let previous_applied = active_provider
            .and_then(|provider| self.state.providers.get(provider_key(provider)))
            .and_then(|state| state.applied_payload.clone());
        for provider in self.state.providers.values_mut() {
            provider.applied_sha256 = None;
            provider.applied_payload = None;
        }
        self.state.active_provider = None;
        self.state.health = Some(unchecked_health(self.state.preferred_provider, false));
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            return Err(error);
        }
        if let Some(previous) = previous_applied {
            remove_snapshot(&self.backups_dir, &previous);
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        flush_dns_best_effort();
        Ok(OperationAccepted { operation_id })
    }

    pub(crate) fn restore(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        let provider = request.provider;
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        let previous_managed = self.active_managed_snapshot()?;
        let (lkg, verified_health) = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| {
                Some((
                    state.verified_last_known_good.clone()?,
                    state.verified_health.clone()?,
                ))
            })
            .ok_or(BackendError::Conflict)?;
        if !controls_available(self.prober.clone()) {
            return Err(BackendError::RuntimeFailed);
        }
        let lkg_bytes = read_snapshot(&self.backups_dir, &lkg).map_err(map_io)?;
        let prepared = merge_preserving_user_entries(
            managed_payload(&lkg_bytes),
            &current,
            previous_managed.as_deref(),
            true,
        )?;
        let before_commit = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if sha256_hex(&before_commit) != sha256_hex(&current) {
            return Err(BackendError::Conflict);
        }
        let operation_id = self.begin_transaction(&current)?;
        let transaction_snapshot = self.pending_backup().clone();
        if let Err(error) = atomic_write(&self.hosts_path, &prepared) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let readback = fs::read(&self.hosts_path).map_err(map_io)?;
        if sha256_hex(&readback) != sha256_hex(&prepared) {
            let _ = self.rollback_pending();
            return Err(BackendError::RuntimeFailed);
        }
        flush_dns_best_effort();
        let plans = plans_from_cached_health(Some(&verified_health), provider);
        let health = verify_post_write_routes(self.prober.clone(), provider, &plans);
        if !post_write_health_is_valid(&plans, &health)
            || health
                .services
                .iter()
                .any(|service| service.health != AiRouteHealth::Working)
        {
            let _ = self.rollback_pending();
            flush_dns_best_effort();
            return Err(BackendError::RuntimeFailed);
        }
        let applied = match self.write_snapshot(&format!("applied-{operation_id}"), &lkg_bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = self.rollback_pending();
                return Err(error);
            }
        };
        let state_before_commit = self.state.clone();
        let previous_applied = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.applied_payload.clone());
        let provider_state = self
            .state
            .providers
            .entry(provider_key(provider).to_owned())
            .or_default();
        provider_state.applied_sha256 = Some(sha256_hex(&prepared));
        provider_state.applied_payload = Some(applied.clone());
        self.state.active_provider = Some(provider);
        self.state.preferred_provider = provider;
        self.state.health = Some(health);
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            remove_snapshot(&self.backups_dir, &applied);
            return Err(error);
        }
        if let Some(previous) = previous_applied.filter(|old| old.file != applied.file) {
            remove_snapshot(&self.backups_dir, &previous);
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        Ok(OperationAccepted { operation_id })
    }

    fn active_managed_snapshot(&self) -> Result<Option<Vec<u8>>, BackendError> {
        let Some(provider) = self.state.active_provider else {
            return Ok(None);
        };
        let snapshot = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.applied_payload.as_ref())
            .ok_or(BackendError::ProtectedResourceInvalid)?;
        read_snapshot(&self.backups_dir, snapshot)
            .map(Some)
            .map_err(map_io)
    }

    fn begin_transaction(&mut self, current: &[u8]) -> Result<u64, BackendError> {
        let operation_id = self.next_operation_id();
        let snapshot = self.write_snapshot(&format!("preop-{operation_id}"), current)?;
        self.state.pending_rollback = Some(snapshot);
        self.save_state()?;
        Ok(operation_id)
    }

    fn recover_interrupted_transaction(&mut self) -> Result<(), BackendError> {
        if self.state.pending_rollback.is_none() {
            return Ok(());
        }
        self.rollback_pending()
    }

    fn rollback_pending(&mut self) -> Result<(), BackendError> {
        let snapshot = self
            .state
            .pending_rollback
            .clone()
            .ok_or(BackendError::Internal)?;
        restore_snapshot(&self.hosts_path, &self.backups_dir, &snapshot).map_err(map_io)?;
        self.state.pending_rollback = None;
        self.save_state()?;
        remove_snapshot(&self.backups_dir, &snapshot);
        Ok(())
    }

    fn pending_backup(&self) -> &SnapshotRef {
        self.state
            .pending_rollback
            .as_ref()
            .expect("pending rollback exists until transaction commit")
    }

    fn write_snapshot(&self, label: &str, bytes: &[u8]) -> Result<SnapshotRef, BackendError> {
        fs::create_dir_all(&self.backups_dir).map_err(map_io)?;
        let nonce = operation_nonce();
        let file = format!("hosts_snapshot_{label}-{}-{nonce}.bin", std::process::id());
        let path = self.backups_dir.join(&file);
        atomic_write(&path, bytes).map_err(map_io)?;
        Ok(SnapshotRef {
            file,
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        })
    }

    fn save_state(&self) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(|_| BackendError::Internal)?;
        atomic_write(&self.state_path, &bytes).map_err(map_io)
    }

    fn next_operation_id(&mut self) -> u64 {
        self.next_operation_id = self.next_operation_id.wrapping_add(1).max(1);
        self.next_operation_id
    }
}

fn merge_preserving_user_entries(
    base: &[u8],
    current: &[u8],
    previous_managed: Option<&[u8]>,
    wrap_user_block: bool,
) -> Result<Vec<u8>, BackendError> {
    if current.len() > MAX_HOSTS_BYTES {
        return Err(BackendError::ProtectedResourceInvalid);
    }

    let base = managed_payload(base);
    let base_domains = collect_domains(base, false);
    let base_lines = collect_line_keys(base, false);
    let previous_domains = previous_managed
        .map(|bytes| collect_domains(bytes, true))
        .unwrap_or_default();
    let previous_lines = previous_managed
        .map(|bytes| collect_line_keys(bytes, true))
        .unwrap_or_default();
    let mut seen_domains = BTreeSet::new();
    let mut seen_lines = BTreeSet::new();
    let mut preserved = Vec::<Vec<u8>>::new();

    for raw_line in current.split(|byte| *byte == b'\n') {
        let line = trim_ascii_line(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
        if line.is_empty() || is_obsession_marker(line) || previous_lines.contains(line) {
            continue;
        }

        if line.starts_with(b"#") {
            if !base_lines.contains(line) && seen_lines.insert(line.to_vec()) {
                preserved.push(line.to_vec());
            }
            continue;
        }

        let Ok(text) = std::str::from_utf8(line) else {
            if !base_lines.contains(line) && seen_lines.insert(line.to_vec()) {
                preserved.push(line.to_vec());
            }
            continue;
        };
        let mut tokens = text.split_whitespace();
        let Some(address) = tokens.next() else {
            continue;
        };
        if address.parse::<IpAddr>().is_err() {
            if !base_lines.contains(line) && seen_lines.insert(line.to_vec()) {
                preserved.push(line.to_vec());
            }
            continue;
        }

        let mut domains = Vec::new();
        let mut malformed = false;
        for domain in tokens.take_while(|token| !token.starts_with('#')) {
            if !valid_domain(domain) {
                malformed = true;
                break;
            }
            let key = domain.to_ascii_lowercase();
            if previous_domains.contains(&key)
                || base_domains.contains(&key)
                || !seen_domains.insert(key)
            {
                continue;
            }
            domains.push(domain);
        }
        if malformed {
            if !base_lines.contains(line) && seen_lines.insert(line.to_vec()) {
                preserved.push(line.to_vec());
            }
        } else if !domains.is_empty() {
            preserved.push(format!("{address} {}", domains.join(" ")).into_bytes());
        }
    }

    if preserved.is_empty() {
        return Ok(base.to_vec());
    }

    let extra_len = preserved.iter().map(|line| line.len() + 1).sum::<usize>()
        + if wrap_user_block {
            USER_BLOCK_BEGIN.len() + USER_BLOCK_END.len() + 2
        } else {
            0
        };
    if base.len().saturating_add(extra_len).saturating_add(1) > MAX_HOSTS_BYTES {
        return Err(BackendError::ProtectedResourceInvalid);
    }

    let mut merged = Vec::with_capacity(base.len() + extra_len + 1);
    merged.extend_from_slice(base);
    if !merged.is_empty() && !merged.ends_with(b"\n") {
        merged.push(b'\n');
    }
    if wrap_user_block {
        merged.extend_from_slice(USER_BLOCK_BEGIN.as_bytes());
        merged.push(b'\n');
    }
    for line in preserved {
        merged.extend_from_slice(&line);
        merged.push(b'\n');
    }
    if wrap_user_block {
        merged.extend_from_slice(USER_BLOCK_END.as_bytes());
        merged.push(b'\n');
    }
    Ok(merged)
}

fn managed_payload(bytes: &[u8]) -> &[u8] {
    let marker = USER_BLOCK_BEGIN.as_bytes();
    let Some(position) = bytes
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return bytes;
    };
    let mut end = position;
    while end > 0 && matches!(bytes[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    &bytes[..end]
}

fn collect_domains(bytes: &[u8], stop_at_user_block: bool) -> BTreeSet<String> {
    let mut domains = BTreeSet::new();
    for raw_line in bytes.split(|byte| *byte == b'\n') {
        let line = trim_ascii_line(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
        if stop_at_user_block && line == USER_BLOCK_BEGIN.as_bytes() {
            break;
        }
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        let mut tokens = text.split_whitespace();
        if !tokens
            .next()
            .is_some_and(|address| address.parse::<IpAddr>().is_ok())
        {
            continue;
        }
        for domain in tokens.take_while(|token| !token.starts_with('#')) {
            if valid_domain(domain) {
                domains.insert(domain.to_ascii_lowercase());
            }
        }
    }
    domains
}

fn collect_line_keys(bytes: &[u8], stop_at_user_block: bool) -> BTreeSet<Vec<u8>> {
    let mut lines = BTreeSet::new();
    for raw_line in bytes.split(|byte| *byte == b'\n') {
        let line = trim_ascii_line(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
        if stop_at_user_block && line == USER_BLOCK_BEGIN.as_bytes() {
            break;
        }
        if !line.is_empty() {
            lines.insert(line.to_vec());
        }
    }
    lines
}

fn trim_ascii_line(mut line: &[u8]) -> &[u8] {
    while line.first().is_some_and(u8::is_ascii_whitespace) {
        line = &line[1..];
    }
    while line.last().is_some_and(u8::is_ascii_whitespace) {
        line = &line[..line.len() - 1];
    }
    line
}

fn is_obsession_marker(line: &[u8]) -> bool {
    line.starts_with(MARKER_PREFIX.as_bytes())
        || line == USER_BLOCK_BEGIN.as_bytes()
        || line == USER_BLOCK_END.as_bytes()
}

fn provider_url(provider: HostsProvider) -> &'static str {
    match provider {
        HostsProvider::Malw => {
            "https://raw.githubusercontent.com/ImMALWARE/dns.malw.link/refs/heads/master/hosts"
        }
        HostsProvider::Geohide => {
            "https://github.com/Internet-Helper/GeoHideDNS/raw/refs/heads/main/hosts/hosts"
        }
    }
}

fn provider_key(provider: HostsProvider) -> &'static str {
    match provider {
        HostsProvider::Malw => "malw",
        HostsProvider::Geohide => "geohide",
    }
}

fn marker(provider: HostsProvider) -> String {
    format!("{MARKER_PREFIX}{}", provider_key(provider))
}

fn contains_marker(bytes: &[u8], provider: HostsProvider) -> bool {
    std::str::from_utf8(bytes)
        .map(|content| content.contains(&marker(provider)))
        .unwrap_or(false)
}

fn detect_managed_provider(bytes: &[u8]) -> Option<HostsProvider> {
    [HostsProvider::Malw, HostsProvider::Geohide]
        .into_iter()
        .find(|provider| contains_marker(bytes, *provider))
}

fn prepare_payload(
    provider: HostsProvider,
    downloaded: &[u8],
) -> Result<PreparedFeed, BackendError> {
    if downloaded.is_empty() || downloaded.len() > MAX_HOSTS_BYTES || downloaded.contains(&0) {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    let text =
        std::str::from_utf8(downloaded).map_err(|_| BackendError::ProtectedResourceInvalid)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut entries = 0usize;
    let mut ipv4 = BTreeMap::<String, Vec<Ipv4Addr>>::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let Some(address) = tokens.next().and_then(|token| token.parse::<IpAddr>().ok()) else {
            continue;
        };
        let mut domains_on_line = 0usize;
        for domain in tokens.take_while(|token| !token.starts_with('#')) {
            if !valid_domain(domain) {
                return Err(BackendError::ProtectedResourceInvalid);
            }
            domains_on_line += 1;
            let domain = domain.to_ascii_lowercase();
            if let IpAddr::V4(address) = address {
                let candidates = ipv4.entry(domain).or_default();
                if !candidates.contains(&address) {
                    candidates.push(address);
                }
            }
        }
        if domains_on_line == 0 {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        entries += 1;
    }
    if entries == 0 {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    if marker(provider).len() + normalized.len() + 2 > MAX_HOSTS_BYTES {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    Ok(PreparedFeed {
        provider,
        normalized,
        ipv4,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ProbeKey {
    host: String,
    address: Option<Ipv4Addr>,
}

#[derive(Clone)]
struct ProbeJob {
    key: ProbeKey,
    path: &'static str,
}

fn execute_probe_jobs(
    prober: Arc<dyn HostsRouteProber>,
    jobs: Vec<ProbeJob>,
) -> BTreeMap<ProbeKey, ProbeOutcome> {
    if jobs.is_empty() {
        return BTreeMap::new();
    }
    let started = Instant::now();
    let queue = Arc::new(Mutex::new(
        jobs.iter().cloned().enumerate().collect::<VecDeque<_>>(),
    ));
    let results = Arc::new(Mutex::new(vec![None; jobs.len()]));
    let workers = MAX_PARALLEL_PROBES.min(jobs.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = queue.clone();
            let results = results.clone();
            let prober = prober.clone();
            scope.spawn(move || loop {
                if started.elapsed() >= PROBE_BUDGET {
                    break;
                }
                let Some((index, job)) = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pop_front()
                else {
                    break;
                };
                let outcome = prober.probe(&job.key.host, job.path, job.key.address);
                results
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)[index] = Some(outcome);
            });
        }
    });
    let results = results
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    jobs.into_iter()
        .enumerate()
        .map(|(index, job)| {
            (
                job.key,
                results[index].unwrap_or(ProbeOutcome {
                    working: false,
                    reason: Some(AiRouteFailureReason::Timeout),
                    elapsed_millis: u64::MAX,
                }),
            )
        })
        .collect()
}

fn controls_available(prober: Arc<dyn HostsRouteProber>) -> bool {
    let jobs = CONTROL_TARGETS
        .iter()
        .map(|target| ProbeJob {
            key: ProbeKey {
                host: target.host.to_owned(),
                address: None,
            },
            path: target.path,
        })
        .collect();
    execute_probe_jobs(prober, jobs)
        .values()
        .any(|outcome| outcome.working)
}

fn plan_service_routes(
    prober: Arc<dyn HostsRouteProber>,
    preferred_provider: HostsProvider,
    preferred: &PreparedFeed,
    fallback: &PreparedFeed,
) -> Vec<ServiceRoutePlan> {
    let mut jobs = BTreeMap::<ProbeKey, ProbeJob>::new();
    for feed in [preferred, fallback] {
        for service in all_services() {
            for target in probe_targets(service) {
                if let Some(candidates) = feed.ipv4.get(target.host) {
                    for address in candidates
                        .iter()
                        .copied()
                        .filter(|address| is_probeable_ipv4(*address))
                        .take(MAX_PROBE_CANDIDATES_PER_DOMAIN)
                    {
                        let key = ProbeKey {
                            host: target.host.to_owned(),
                            address: Some(address),
                        };
                        jobs.entry(key.clone()).or_insert(ProbeJob {
                            key,
                            path: target.path,
                        });
                    }
                }
            }
        }
    }
    let results = execute_probe_jobs(prober, jobs.into_values().collect());
    all_services()
        .into_iter()
        .map(|service| {
            if let Some(selected_candidates) = viable_feed_candidates(preferred, service, &results)
            {
                return ServiceRoutePlan {
                    service,
                    route: AiRouteKind::Preferred,
                    provider: Some(preferred_provider),
                    selected_candidates,
                };
            }
            if let Some(selected_candidates) = viable_feed_candidates(fallback, service, &results) {
                return ServiceRoutePlan {
                    service,
                    route: AiRouteKind::Fallback,
                    provider: Some(fallback.provider),
                    selected_candidates,
                };
            }
            ServiceRoutePlan {
                service,
                route: AiRouteKind::Direct,
                provider: None,
                selected_candidates: BTreeMap::new(),
            }
        })
        .collect()
}

fn viable_feed_candidates(
    feed: &PreparedFeed,
    service: AiService,
    results: &BTreeMap<ProbeKey, ProbeOutcome>,
) -> Option<BTreeMap<String, Ipv4Addr>> {
    let mut selected = BTreeMap::new();
    for target in probe_targets(service) {
        let address = feed
            .ipv4
            .get(target.host)?
            .iter()
            .copied()
            .filter(|address| is_probeable_ipv4(*address))
            .take(MAX_PROBE_CANDIDATES_PER_DOMAIN)
            .filter_map(|address| {
                let outcome = results.get(&ProbeKey {
                    host: target.host.to_owned(),
                    address: Some(address),
                })?;
                outcome.working.then_some((outcome.elapsed_millis, address))
            })
            .min_by_key(|(elapsed_millis, _)| *elapsed_millis)?
            .1;
        selected.insert(target.host.to_owned(), address);
    }
    Some(selected)
}

fn render_hybrid_payload(
    preferred: &PreparedFeed,
    fallback: &PreparedFeed,
    plans: &[ServiceRoutePlan],
) -> Result<Vec<u8>, BackendError> {
    let mut output = String::with_capacity(preferred.normalized.len());
    output.push_str(&marker(preferred.provider));
    output.push('\n');
    for raw_line in preferred.normalized.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            output.push_str(line);
            output.push('\n');
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(address) = tokens.next().and_then(|value| value.parse::<IpAddr>().ok()) else {
            continue;
        };
        let domains = tokens
            .take_while(|token| !token.starts_with('#'))
            .filter(|domain| service_for_domain(domain).is_none())
            .collect::<Vec<_>>();
        if !domains.is_empty() {
            output.push_str(&address.to_string());
            output.push(' ');
            output.push_str(&domains.join(" "));
            output.push('\n');
        }
    }
    output.push_str("# obsession:verified-ai-routes\n");
    for plan in plans {
        let Some(provider) = plan.provider else {
            continue;
        };
        let feed = if provider == preferred.provider {
            preferred
        } else {
            fallback
        };
        for (domain, candidates) in &feed.ipv4 {
            if service_for_domain(domain) != Some(plan.service) {
                continue;
            }
            let selected = plan
                .selected_candidates
                .get(domain)
                .copied()
                .or_else(|| {
                    candidates.iter().copied().find(|address| {
                        plan.selected_candidates
                            .values()
                            .any(|selected| selected == address && is_probeable_ipv4(*address))
                    })
                })
                .or_else(|| {
                    candidates
                        .iter()
                        .copied()
                        .find(|address| is_probeable_ipv4(*address))
                });
            if let Some(address) = selected {
                output.push_str(&format!("{address} {domain}\n"));
            }
        }
    }
    if output.len() > MAX_HOSTS_BYTES {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    Ok(output.into_bytes())
}

fn plans_from_cached_health(
    health: Option<&HostsHealthSnapshot>,
    preferred_provider: HostsProvider,
) -> Vec<ServiceRoutePlan> {
    all_services()
        .into_iter()
        .map(|service| {
            let cached = health.and_then(|health| {
                health
                    .services
                    .iter()
                    .find(|entry| entry.service == service)
            });
            ServiceRoutePlan {
                service,
                route: cached.map_or(AiRouteKind::Preferred, |entry| entry.route),
                provider: cached
                    .and_then(|entry| entry.provider)
                    .or(Some(preferred_provider))
                    .filter(|_| !cached.is_some_and(|entry| entry.route == AiRouteKind::Direct)),
                selected_candidates: BTreeMap::new(),
            }
        })
        .collect()
}

fn check_system_routes(
    prober: Arc<dyn HostsRouteProber>,
    preferred_provider: HostsProvider,
    plans: &[ServiceRoutePlan],
) -> HostsHealthSnapshot {
    let mut jobs = Vec::new();
    for plan in plans.iter().filter(|plan| plan.provider.is_some()) {
        for target in probe_targets(plan.service) {
            jobs.push(ProbeJob {
                key: ProbeKey {
                    host: target.host.to_owned(),
                    address: None,
                },
                path: target.path,
            });
        }
    }
    let results = execute_probe_jobs(prober, jobs);
    let services = plans
        .iter()
        .map(|plan| {
            let Some(provider) = plan.provider else {
                return AiServiceRouteHealth {
                    service: plan.service,
                    health: AiRouteHealth::Unavailable,
                    route: AiRouteKind::Direct,
                    provider: None,
                    reason: Some(AiRouteFailureReason::RouteMissing),
                };
            };
            let failed = probe_targets(plan.service).iter().find_map(|target| {
                results
                    .get(&ProbeKey {
                        host: target.host.to_owned(),
                        address: None,
                    })
                    .filter(|outcome| !outcome.working)
                    .copied()
            });
            AiServiceRouteHealth {
                service: plan.service,
                health: if failed.is_none() {
                    AiRouteHealth::Working
                } else {
                    AiRouteHealth::Unavailable
                },
                route: plan.route,
                provider: Some(provider),
                reason: failed.and_then(|outcome| outcome.reason),
            }
        })
        .collect::<Vec<_>>();
    HostsHealthSnapshot {
        preferred_provider,
        installed: true,
        checked_at_unix: Some(unix_now()),
        repair_recommended: services
            .iter()
            .any(|service| service.health == AiRouteHealth::Unavailable),
        services,
    }
}

fn stabilize_route_health(
    previous: Option<&HostsHealthSnapshot>,
    mut fresh: HostsHealthSnapshot,
) -> HostsHealthSnapshot {
    let Some(previous) = previous.filter(|snapshot| {
        snapshot.installed == fresh.installed
            && snapshot.preferred_provider == fresh.preferred_provider
    }) else {
        return fresh;
    };

    for service in &mut fresh.services {
        if service.health != AiRouteHealth::Unavailable {
            continue;
        }
        let was_working = previous.services.iter().any(|entry| {
            entry.service == service.service
                && entry.health == AiRouteHealth::Working
                && entry.route == service.route
                && entry.provider == service.provider
        });
        if was_working {
            service.health = AiRouteHealth::Inconclusive;
        }
    }
    fresh.repair_recommended = fresh
        .services
        .iter()
        .any(|service| service.health == AiRouteHealth::Unavailable);
    fresh
}

fn post_write_health_is_valid(plans: &[ServiceRoutePlan], health: &HostsHealthSnapshot) -> bool {
    plans.iter().all(|plan| {
        plan.provider.is_none()
            || health.services.iter().any(|entry| {
                entry.service == plan.service && entry.health == AiRouteHealth::Working
            })
    })
}

fn verify_post_write_routes(
    prober: Arc<dyn HostsRouteProber>,
    preferred_provider: HostsProvider,
    plans: &[ServiceRoutePlan],
) -> HostsHealthSnapshot {
    wait_for_dns_settle(POST_WRITE_DNS_SETTLE_DELAY);
    let mut health = check_system_routes(prober.clone(), preferred_provider, plans);
    for delay in POST_WRITE_RETRY_DELAYS {
        if post_write_health_is_valid(plans, &health) {
            return health;
        }
        // The Windows DNS Client reparses the entire hosts file after a flush.
        // Flushing again here restarts that work and can keep every lookup in
        // a permanent resolving timeout for large provider payloads.
        wait_for_dns_settle(delay);
        health = check_system_routes(prober.clone(), preferred_provider, plans);
    }
    health
}

fn wait_for_dns_settle(delay: Duration) {
    #[cfg(not(test))]
    std::thread::sleep(delay);

    #[cfg(test)]
    let _ = delay;
}

fn uniform_health(
    preferred_provider: HostsProvider,
    installed: bool,
    health: AiRouteHealth,
    reason: Option<AiRouteFailureReason>,
    repair_recommended: bool,
) -> HostsHealthSnapshot {
    HostsHealthSnapshot {
        preferred_provider,
        installed,
        checked_at_unix: Some(unix_now()),
        repair_recommended,
        services: all_services()
            .into_iter()
            .map(|service| AiServiceRouteHealth {
                service,
                health,
                route: if installed {
                    AiRouteKind::Preferred
                } else {
                    AiRouteKind::Direct
                },
                provider: installed.then_some(preferred_provider),
                reason,
            })
            .collect(),
    }
}

fn uniform_health_for_plans(
    preferred_provider: HostsProvider,
    installed: bool,
    plans: &[ServiceRoutePlan],
    health: AiRouteHealth,
    reason: Option<AiRouteFailureReason>,
    repair_recommended: bool,
) -> HostsHealthSnapshot {
    HostsHealthSnapshot {
        preferred_provider,
        installed,
        checked_at_unix: Some(unix_now()),
        repair_recommended,
        services: plans
            .iter()
            .map(|plan| AiServiceRouteHealth {
                service: plan.service,
                health,
                route: plan.route,
                provider: plan.provider,
                reason,
            })
            .collect(),
    }
}

fn unchecked_health(preferred_provider: HostsProvider, installed: bool) -> HostsHealthSnapshot {
    let mut snapshot = uniform_health(
        preferred_provider,
        installed,
        AiRouteHealth::Unchecked,
        None,
        false,
    );
    snapshot.checked_at_unix = None;
    snapshot
}

fn is_probeable_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && matches!(b, 18 | 19 | 51))
        || (a == 203 && b == 0 && c == 113))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && !domain.starts_with('.')
        && !domain.starts_with('-')
        && !domain.ends_with('-')
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn extract_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    for line in text.lines().take(128) {
        let comment = line.trim().strip_prefix('#')?.trim();
        for prefix in ["update:", "Последнее обновление:"] {
            if let Some(value) = comment.strip_prefix(prefix).map(str::trim) {
                if !value.is_empty() && value.len() <= MAX_VERSION_BYTES {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

fn load_state(path: &Path) -> HostsManagedState {
    let Ok(bytes) = fs::read(path) else {
        return HostsManagedState::default();
    };
    if let Ok(state) = serde_json::from_slice::<HostsManagedState>(&bytes) {
        if state.schema_version == STATE_SCHEMA_VERSION {
            return state;
        }
    }
    let Ok(legacy) = serde_json::from_slice::<LegacyHostsManagedState>(&bytes) else {
        return HostsManagedState::default();
    };
    if legacy.schema_version != 1 {
        return HostsManagedState::default();
    }
    let preferred_provider = legacy.active_provider.unwrap_or(HostsProvider::Malw);
    let providers = legacy
        .providers
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                ProviderState {
                    applied_sha256: value.applied_sha256,
                    applied_payload: value.last_known_good,
                    verified_last_known_good: None,
                    verified_health: None,
                },
            )
        })
        .collect();
    HostsManagedState {
        schema_version: STATE_SCHEMA_VERSION,
        original: legacy.original,
        providers,
        active_provider: legacy.active_provider,
        preferred_provider,
        health: None,
        pending_rollback: legacy.pending_rollback,
    }
}

fn read_snapshot(backups_dir: &Path, snapshot: &SnapshotRef) -> io::Result<Vec<u8>> {
    if !valid_snapshot_name(&snapshot.file) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid hosts snapshot name",
        ));
    }
    let bytes = fs::read(backups_dir.join(&snapshot.file))?;
    if bytes.len() as u64 != snapshot.size || sha256_hex(&bytes) != snapshot.sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "hosts snapshot integrity mismatch",
        ));
    }
    Ok(bytes)
}

fn restore_snapshot(
    hosts_path: &Path,
    backups_dir: &Path,
    snapshot: &SnapshotRef,
) -> io::Result<()> {
    let bytes = read_snapshot(backups_dir, snapshot)?;
    atomic_write(hosts_path, &bytes)?;
    let readback = fs::read(hosts_path)?;
    if sha256_hex(&readback) != snapshot.sha256 {
        return Err(io::Error::other("hosts restore verification failed"));
    }
    Ok(())
}

fn find_orphaned_original(backups_dir: &Path) -> Option<SnapshotRef> {
    let mut files = fs::read_dir(backups_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_str()?.to_owned();
            (file.starts_with("hosts_snapshot_original-") && valid_snapshot_name(&file))
                .then_some(file)
        })
        .collect::<Vec<_>>();
    files.sort();
    let file = files.into_iter().next()?;
    let bytes = fs::read(backups_dir.join(&file)).ok()?;
    Some(SnapshotRef {
        file,
        sha256: sha256_hex(&bytes),
        size: bytes.len() as u64,
    })
}

fn valid_snapshot_name(file: &str) -> bool {
    file.starts_with("hosts_snapshot_")
        && file.ends_with(".bin")
        && file.len() <= 160
        && file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn remove_snapshot(backups_dir: &Path, snapshot: &SnapshotRef) {
    if valid_snapshot_name(&snapshot.file) {
        let _ = fs::remove_file(backups_dir.join(&snapshot.file));
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut last_error = match atomic_write_once(path, bytes) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    for delay_ms in ATOMIC_WRITE_RETRY_DELAYS_MS {
        std::thread::sleep(Duration::from_millis(delay_ms));
        match atomic_write_once(path, bytes) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn atomic_write_once(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid filename"))?;
    let temp = parent.join(format!(".{file_name}.obsession-{}.tmp", operation_nonce()));
    let write_result = (|| -> io::Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let from = wide_path(&temp);
    let to = wide_path(path);
    let result = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(io::Error::other(error.to_string()));
    }
    let readback = fs::read(path)?;
    if readback != bytes {
        return Err(io::Error::other("atomic write verification failed"));
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn operation_nonce() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    millis
        .wrapping_mul(1_000)
        .wrapping_add(OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed) % 1_000)
}

fn path_eq(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn map_io(error: io::Error) -> BackendError {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            BackendError::ProtectedResourceInvalid
        }
        io::ErrorKind::PermissionDenied => BackendError::ServiceUnavailable,
        _ => BackendError::RuntimeFailed,
    }
}

fn flush_dns_best_effort() {
    #[cfg(test)]
    return;

    #[cfg(not(test))]
    {
        let Some(system_root) = std::env::var_os("SystemRoot") else {
            return;
        };
        let _ = Command::new(PathBuf::from(system_root).join("System32/ipconfig.exe"))
            .arg("/flushdns")
            .creation_flags(CREATE_NO_WINDOW_FLAG)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticDownloader(Vec<u8>);

    impl HostsDownloader for StaticDownloader {
        fn download(&self, _provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
            Ok(self.0.clone())
        }
    }

    struct MutatingDownloader {
        hosts_path: PathBuf,
        payload: Vec<u8>,
    }

    impl HostsDownloader for MutatingDownloader {
        fn download(&self, _provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
            fs::write(&self.hosts_path, b"127.0.0.1 changed-during-download\n").unwrap();
            Ok(self.payload.clone())
        }
    }

    struct StaticProber;

    impl HostsRouteProber for StaticProber {
        fn probe(&self, _host: &str, _path: &str, _address: Option<Ipv4Addr>) -> ProbeOutcome {
            ProbeOutcome {
                working: true,
                reason: None,
                elapsed_millis: 1,
            }
        }
    }

    struct ProviderDownloader {
        malw: Vec<u8>,
        geohide: Vec<u8>,
    }

    impl HostsDownloader for ProviderDownloader {
        fn download(&self, provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
            Ok(match provider {
                HostsProvider::Malw => self.malw.clone(),
                HostsProvider::Geohide => self.geohide.clone(),
            })
        }
    }

    struct ScriptedProber {
        controls_online: bool,
        working_addresses: BTreeSet<Ipv4Addr>,
        system_failures: BTreeSet<String>,
    }

    struct RankedProber(BTreeMap<Ipv4Addr, u64>);

    impl HostsRouteProber for RankedProber {
        fn probe(&self, _host: &str, _path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome {
            let elapsed_millis = address
                .and_then(|address| self.0.get(&address).copied())
                .unwrap_or(1);
            ProbeOutcome {
                working: address.is_none() || self.0.contains_key(&address.unwrap()),
                reason: address
                    .filter(|address| !self.0.contains_key(address))
                    .map(|_| AiRouteFailureReason::Timeout),
                elapsed_millis,
            }
        }
    }

    struct TransientSystemProber {
        gemini_system_attempts: std::sync::atomic::AtomicUsize,
    }

    struct SwitchableSystemProber {
        fail_chatgpt: std::sync::atomic::AtomicBool,
    }

    impl HostsRouteProber for TransientSystemProber {
        fn probe(&self, host: &str, _path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome {
            if CONTROL_TARGETS.iter().any(|target| target.host == host) || address.is_some() {
                return ProbeOutcome {
                    working: true,
                    reason: None,
                    elapsed_millis: 1,
                };
            }
            let transient_failure = host == "gemini.google.com"
                && self.gemini_system_attempts.fetch_add(1, Ordering::SeqCst) == 0;
            ProbeOutcome {
                working: !transient_failure,
                reason: transient_failure.then_some(AiRouteFailureReason::Timeout),
                elapsed_millis: 1,
            }
        }
    }

    impl HostsRouteProber for SwitchableSystemProber {
        fn probe(&self, host: &str, _path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome {
            let working = CONTROL_TARGETS.iter().any(|target| target.host == host)
                || address.is_some()
                || host != "chatgpt.com"
                || !self.fail_chatgpt.load(Ordering::SeqCst);
            ProbeOutcome {
                working,
                reason: (!working).then_some(AiRouteFailureReason::Timeout),
                elapsed_millis: 1,
            }
        }
    }

    impl HostsRouteProber for ScriptedProber {
        fn probe(&self, host: &str, _path: &str, address: Option<Ipv4Addr>) -> ProbeOutcome {
            if CONTROL_TARGETS.iter().any(|target| target.host == host) {
                return ProbeOutcome {
                    working: self.controls_online,
                    reason: (!self.controls_online).then_some(AiRouteFailureReason::Timeout),
                    elapsed_millis: 1,
                };
            }
            let working = address.map_or_else(
                || !self.system_failures.contains(host),
                |address| self.working_addresses.contains(&address),
            );
            ProbeOutcome {
                working,
                reason: (!working).then_some(AiRouteFailureReason::Timeout),
                elapsed_millis: 1,
            }
        }
    }

    fn feed_for(chatgpt: Ipv4Addr, claude: Ipv4Addr, gemini: Ipv4Addr) -> Vec<u8> {
        format!(
            "{chatgpt} chatgpt.com\n{chatgpt} auth.openai.com\n{chatgpt} cdn.oaistatic.com\n\
             {claude} claude.ai\n{claude} api.anthropic.com\n\
             {gemini} gemini.google.com\n{gemini} aistudio.google.com\n\
             {gemini} generativelanguage.googleapis.com\n"
        )
        .into_bytes()
    }

    fn scripted_controller(
        root: &Path,
        malw: Vec<u8>,
        geohide: Vec<u8>,
        prober: ScriptedProber,
    ) -> HostsController {
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\n").unwrap();
        HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(ProviderDownloader { malw, geohide }),
            Arc::new(prober),
        )
        .unwrap()
    }

    fn temp_root(name: &str) -> PathBuf {
        let suffix = operation_nonce();
        let root = std::env::temp_dir().join(format!("obsession-service-hosts-{name}-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn controller(root: &Path, payload: &[u8]) -> HostsController {
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\r\n").unwrap();
        let mut complete_payload = payload.to_vec();
        complete_payload.extend_from_slice(
            b"\n1.2.3.4 chatgpt.com\n1.2.3.4 auth.openai.com\n1.2.3.4 cdn.oaistatic.com\n\
              1.2.3.4 claude.ai\n1.2.3.4 api.anthropic.com\n1.2.3.4 gemini.google.com\n\
              1.2.3.4 aistudio.google.com\n1.2.3.4 generativelanguage.googleapis.com\n",
        );
        HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(StaticDownloader(complete_payload)),
            Arc::new(StaticProber),
        )
        .unwrap()
    }

    #[test]
    fn atomic_write_retries_a_transient_replace_failure() {
        let root = temp_root("atomic-retry");
        let target = root.join("hosts");
        fs::create_dir(&target).unwrap();
        let blocked_target = target.clone();
        let unblock = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            fs::remove_dir(blocked_target).unwrap();
        });

        atomic_write(&target, b"127.0.0.1 localhost\n").unwrap();
        unblock.join().unwrap();
        assert_eq!(fs::read(target).unwrap(), b"127.0.0.1 localhost\n");
    }

    #[test]
    fn install_reconciles_external_entries_without_losing_them() {
        let root = temp_root("install");
        let mut controller = controller(&root, b"# update: 2026-08-04\n1.2.3.4 chatgpt.com\n");
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        let snapshot = controller.snapshot().unwrap();
        assert!(snapshot.installed);
        assert_eq!(snapshot.local_version.as_deref(), Some("2026-08-04"));
        assert!(snapshot.rollback_available);

        fs::write(&controller.hosts_path, b"127.0.0.1 external-change\n").unwrap();
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        let installed = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(installed.contains("1.2.3.4 chatgpt.com"));
        assert!(installed.contains("127.0.0.1 external-change"));
        assert!(installed.contains(USER_BLOCK_BEGIN));
        assert!(!controller.snapshot().unwrap().externally_modified);
    }

    #[test]
    fn install_replaces_managed_domains_but_keeps_unrelated_entries() {
        let root = temp_root("managed-replacement");
        let mut controller = controller(&root, b"1.2.3.4 chatgpt.com\n");
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        fs::write(
            &controller.hosts_path,
            b"9.9.9.9 chatgpt.com\n127.0.0.1 custom.local\n",
        )
        .unwrap();

        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        let installed = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(installed.contains("1.2.3.4 chatgpt.com"));
        assert!(!installed.contains("9.9.9.9 chatgpt.com"));
        assert!(installed.contains("127.0.0.1 custom.local"));
    }

    #[test]
    fn install_still_fails_closed_when_hosts_changes_during_download() {
        let root = temp_root("concurrent-change");
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\n").unwrap();
        let mut controller = HostsController::from_paths(
            hosts_path.clone(),
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(MutatingDownloader {
                hosts_path,
                payload: b"1.2.3.4 chatgpt.com\n1.2.3.4 auth.openai.com\n1.2.3.4 cdn.oaistatic.com\n1.2.3.4 claude.ai\n1.2.3.4 api.anthropic.com\n1.2.3.4 gemini.google.com\n1.2.3.4 aistudio.google.com\n1.2.3.4 generativelanguage.googleapis.com\n".to_vec(),
            }),
            Arc::new(StaticProber),
        )
        .unwrap();

        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::Conflict)
        );
    }

    #[test]
    fn uninstall_restores_the_exact_original_and_lkg_can_be_reapplied() {
        let root = temp_root("rollback");
        let mut controller = controller(&root, b"1.2.3.4 claude.ai\n");
        let original = fs::read(&controller.hosts_path).unwrap();
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        controller.uninstall().unwrap();
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
        assert!(controller.snapshot().is_none());

        controller
            .restore(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        assert!(controller.snapshot().unwrap().installed);
    }

    #[test]
    fn uninstall_preserves_entries_added_after_install() {
        let root = temp_root("uninstall-overlay");
        let mut controller = controller(&root, b"1.2.3.4 claude.ai\n");
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        fs::write(
            &controller.hosts_path,
            b"1.2.3.4 claude.ai\n184.24.230.247 gamelogs.live.bhvrdbd.com\n",
        )
        .unwrap();

        controller.uninstall().unwrap();
        let restored = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(restored.contains("127.0.0.1 localhost"));
        assert!(restored.contains("184.24.230.247 gamelogs.live.bhvrdbd.com"));
        assert!(!restored.contains("claude.ai"));
        assert!(!restored.contains(USER_BLOCK_BEGIN));
    }

    #[test]
    fn restore_reapplies_managed_data_and_preserves_external_entries() {
        let root = temp_root("restore-overlay");
        let mut controller = controller(&root, b"1.2.3.4 claude.ai\n");
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        fs::write(
            &controller.hosts_path,
            b"184.24.230.247 gamelogs.live.bhvrdbd.com\n",
        )
        .unwrap();

        controller
            .restore(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        let restored = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(restored.contains("1.2.3.4 claude.ai"));
        assert!(restored.contains("184.24.230.247 gamelogs.live.bhvrdbd.com"));
        assert!(!controller.snapshot().unwrap().externally_modified);
    }

    #[test]
    fn startup_rolls_back_an_interrupted_system_file_write() {
        let root = temp_root("crash-recovery");
        let mut controller = controller(&root, b"1.2.3.4 gemini.google.com\n");
        let original = fs::read(&controller.hosts_path).unwrap();
        controller.begin_transaction(&original).unwrap();
        atomic_write(&controller.hosts_path, b"broken-partial-state\n").unwrap();

        let recovered = HostsController::from_paths(
            controller.hosts_path.clone(),
            controller.state_path.clone(),
            controller.backups_dir.clone(),
            Arc::new(StaticDownloader(Vec::new())),
            Arc::new(StaticProber),
        )
        .unwrap();
        assert_eq!(fs::read(&recovered.hosts_path).unwrap(), original);
        assert!(recovered.state.pending_rollback.is_none());
    }

    #[test]
    fn geohide_multi_ip_domains_are_preserved_as_ordered_candidates() {
        let feed = prepare_payload(
            HostsProvider::Geohide,
            b"45.155.204.190 gemini.google.com\n37.230.192.51 gemini.google.com\n",
        )
        .unwrap();
        assert_eq!(
            feed.ipv4.get("gemini.google.com").unwrap(),
            &vec![
                Ipv4Addr::new(45, 155, 204, 190),
                Ipv4Addr::new(37, 230, 192, 51),
            ]
        );
    }

    #[test]
    fn route_planner_prefers_the_fastest_stable_candidate_not_feed_order() {
        let dead = Ipv4Addr::new(62, 133, 62, 97);
        let slow = Ipv4Addr::new(45, 155, 204, 190);
        let fast = Ipv4Addr::new(37, 230, 192, 51);
        let preferred = prepare_payload(HostsProvider::Malw, &feed_for(dead, dead, dead)).unwrap();
        let mut fallback_bytes = feed_for(slow, slow, slow);
        fallback_bytes.extend_from_slice(
            format!(
                "{fast} gemini.google.com\n{fast} aistudio.google.com\n\
                 {fast} generativelanguage.googleapis.com\n"
            )
            .as_bytes(),
        );
        let fallback = prepare_payload(HostsProvider::Geohide, &fallback_bytes).unwrap();

        let plans = plan_service_routes(
            Arc::new(RankedProber(BTreeMap::from([(slow, 4_000), (fast, 900)]))),
            HostsProvider::Malw,
            &preferred,
            &fallback,
        );
        let gemini = plans
            .iter()
            .find(|plan| plan.service == AiService::Gemini)
            .unwrap();

        assert_eq!(gemini.route, AiRouteKind::Fallback);
        assert!(gemini
            .selected_candidates
            .values()
            .all(|address| *address == fast));
    }

    #[test]
    fn dead_preferred_gemini_uses_only_geohide_fallback() {
        let root = temp_root("hybrid-gemini");
        let malw_shared = Ipv4Addr::new(45, 155, 204, 190);
        let dead_gemini = Ipv4Addr::new(62, 133, 62, 97);
        let geohide = Ipv4Addr::new(37, 230, 192, 51);
        let unstable = Ipv4Addr::new(46, 1, 1, 1);
        let mut geohide_feed = feed_for(geohide, geohide, unstable);
        geohide_feed.extend_from_slice(
            format!(
                "{geohide} gemini.google.com\n{geohide} aistudio.google.com\n\
                 {geohide} generativelanguage.googleapis.com\n"
            )
            .as_bytes(),
        );
        let mut controller = scripted_controller(
            &root,
            feed_for(malw_shared, malw_shared, dead_gemini),
            geohide_feed,
            ScriptedProber {
                controls_online: true,
                working_addresses: BTreeSet::from([malw_shared, geohide]),
                system_failures: BTreeSet::new(),
            },
        );
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();

        let installed = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(installed.contains("45.155.204.190 chatgpt.com"));
        assert!(installed.contains("45.155.204.190 claude.ai"));
        assert!(installed.contains("37.230.192.51 gemini.google.com"));
        assert!(!installed.contains("62.133.62.97"));
        let health = controller.check(0).unwrap();
        assert_eq!(health.services[0].route, AiRouteKind::Preferred);
        assert_eq!(health.services[1].route, AiRouteKind::Preferred);
        assert_eq!(health.services[2].route, AiRouteKind::Fallback);
        assert!(health
            .services
            .iter()
            .all(|service| service.health == AiRouteHealth::Working));
        assert!(controller.snapshot().unwrap().rollback_available);
    }

    #[test]
    fn both_dead_routes_remove_only_the_failed_service_group() {
        let root = temp_root("hybrid-partial");
        let preferred = Ipv4Addr::new(45, 155, 204, 190);
        let dead_gemini = Ipv4Addr::new(62, 133, 62, 97);
        let dead_fallback = Ipv4Addr::new(37, 230, 192, 51);
        let mut controller = scripted_controller(
            &root,
            feed_for(preferred, preferred, dead_gemini),
            feed_for(dead_fallback, dead_fallback, dead_fallback),
            ScriptedProber {
                controls_online: true,
                working_addresses: BTreeSet::from([preferred]),
                system_failures: BTreeSet::new(),
            },
        );
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();

        let installed = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(installed.contains("chatgpt.com"));
        assert!(installed.contains("claude.ai"));
        assert!(!installed.contains("gemini.google.com"));
        let health = controller.state.health.as_ref().unwrap();
        let gemini = health
            .services
            .iter()
            .find(|service| service.service == AiService::Gemini)
            .unwrap();
        assert_eq!(gemini.route, AiRouteKind::Direct);
        assert_eq!(gemini.health, AiRouteHealth::Unavailable);
        assert!(!controller.snapshot().unwrap().rollback_available);
    }

    #[test]
    fn offline_control_failure_never_writes_hosts() {
        let root = temp_root("hybrid-offline");
        let address = Ipv4Addr::new(45, 155, 204, 190);
        let mut controller = scripted_controller(
            &root,
            feed_for(address, address, address),
            feed_for(address, address, address),
            ScriptedProber {
                controls_online: false,
                working_addresses: BTreeSet::from([address]),
                system_failures: BTreeSet::new(),
            },
        );
        let original = fs::read(&controller.hosts_path).unwrap();
        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::RuntimeFailed)
        );
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
    }

    #[test]
    fn post_write_route_failure_rolls_back_exact_previous_hosts() {
        let root = temp_root("hybrid-post-write");
        let address = Ipv4Addr::new(45, 155, 204, 190);
        let mut controller = scripted_controller(
            &root,
            feed_for(address, address, address),
            feed_for(address, address, address),
            ScriptedProber {
                controls_online: true,
                working_addresses: BTreeSet::from([address]),
                system_failures: BTreeSet::from(["gemini.google.com".to_owned()]),
            },
        );
        let original = fs::read(&controller.hosts_path).unwrap();
        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::RuntimeFailed)
        );
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
        assert!(controller.state.pending_rollback.is_none());
    }

    #[test]
    fn transient_post_write_failure_is_retried_before_commit() {
        let root = temp_root("hybrid-post-write-transient");
        let address = Ipv4Addr::new(45, 155, 204, 190);
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\n").unwrap();
        let feed = feed_for(address, address, address);
        let mut controller = HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(ProviderDownloader {
                malw: feed.clone(),
                geohide: feed,
            }),
            Arc::new(TransientSystemProber {
                gemini_system_attempts: std::sync::atomic::AtomicUsize::new(0),
            }),
        )
        .unwrap();

        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();

        let health = controller.state.health.as_ref().unwrap();
        assert!(health
            .services
            .iter()
            .all(|service| service.health == AiRouteHealth::Working));
        assert!(controller.state.pending_rollback.is_none());
    }

    #[test]
    fn a_single_failed_health_round_is_inconclusive_before_becoming_unavailable() {
        let root = temp_root("hybrid-health-confirmation");
        let address = Ipv4Addr::new(45, 155, 204, 190);
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\n").unwrap();
        let feed = feed_for(address, address, address);
        let prober = Arc::new(SwitchableSystemProber {
            fail_chatgpt: std::sync::atomic::AtomicBool::new(false),
        });
        let mut controller = HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(ProviderDownloader {
                malw: feed.clone(),
                geohide: feed,
            }),
            prober.clone(),
        )
        .unwrap();
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();

        prober.fail_chatgpt.store(true, Ordering::SeqCst);
        let first = controller.check(0).unwrap();
        let chatgpt = first
            .services
            .iter()
            .find(|service| service.service == AiService::Chatgpt)
            .unwrap();
        assert_eq!(chatgpt.health, AiRouteHealth::Inconclusive);
        assert!(!first.repair_recommended);

        let second = controller.check(0).unwrap();
        let chatgpt = second
            .services
            .iter()
            .find(|service| service.service == AiService::Chatgpt)
            .unwrap();
        assert_eq!(chatgpt.health, AiRouteHealth::Unavailable);
        assert!(second.repair_recommended);
    }

    #[test]
    fn private_feed_candidates_are_never_selected_or_probed() {
        let root = temp_root("hybrid-private");
        let public = Ipv4Addr::new(45, 155, 204, 190);
        let private = Ipv4Addr::new(10, 0, 0, 7);
        let mut controller = scripted_controller(
            &root,
            feed_for(public, public, private),
            feed_for(public, public, private),
            ScriptedProber {
                controls_online: true,
                working_addresses: BTreeSet::from([public, private]),
                system_failures: BTreeSet::new(),
            },
        );
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        let installed = fs::read_to_string(&controller.hosts_path).unwrap();
        assert!(!installed.contains("10.0.0.7"));
        assert!(!installed.contains("gemini.google.com"));
    }

    #[test]
    fn v1_state_migrates_lkg_to_unverified_applied_payload() {
        let root = temp_root("state-v1");
        let hosts_path = root.join("hosts");
        let state_path = root.join("state/hosts-state.json");
        let backups = root.join("state/backups");
        fs::create_dir_all(&backups).unwrap();
        let payload = b"# obsession:ai-provider=malw\n1.2.3.4 chatgpt.com\n";
        fs::write(&hosts_path, payload).unwrap();
        let snapshot = SnapshotRef {
            file: "hosts_snapshot_lkg-v1.bin".into(),
            sha256: sha256_hex(payload),
            size: payload.len() as u64,
        };
        fs::write(backups.join(&snapshot.file), payload).unwrap();
        let legacy = LegacyHostsManagedState {
            schema_version: 1,
            original: None,
            providers: BTreeMap::from([(
                "malw".into(),
                LegacyProviderState {
                    applied_sha256: Some(sha256_hex(payload)),
                    last_known_good: Some(snapshot),
                },
            )]),
            active_provider: Some(HostsProvider::Malw),
            pending_rollback: None,
        };
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        fs::write(&state_path, serde_json::to_vec(&legacy).unwrap()).unwrap();

        let controller = HostsController::from_paths(
            hosts_path,
            state_path,
            backups,
            Arc::new(StaticDownloader(Vec::new())),
            Arc::new(StaticProber),
        )
        .unwrap();
        let migrated = controller.state.providers.get("malw").unwrap();
        assert!(migrated.applied_payload.is_some());
        assert!(migrated.verified_last_known_good.is_none());
        assert!(!controller.snapshot().unwrap().rollback_available);
    }

    #[test]
    fn invalid_or_unbounded_payload_never_touches_system_hosts() {
        let root = temp_root("invalid");
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\n").unwrap();
        let mut controller = HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Arc::new(StaticDownloader(b"<html>error</html>\n".to_vec())),
            Arc::new(StaticProber),
        )
        .unwrap();
        let original = fs::read(&controller.hosts_path).unwrap();
        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::ProtectedResourceInvalid)
        );
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
    }
}
