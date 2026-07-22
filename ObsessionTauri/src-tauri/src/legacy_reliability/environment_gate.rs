//! Bounded, side-effect-free decision support for the Legacy Environment Gate.
//!
//! The gate only observes local/network state and returns a typed report. It
//! never starts or stops a process, changes a config, or writes a cache. The
//! HTTP backend is injectable so classification and timing policy remain
//! deterministic in tests.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::lookup_host;
use tokio::task::JoinSet;

use super::contracts::{
    EventEnvelope, EyeHealthState, LaneGeneration, NetworkFingerprint, RegistryVersion,
    SensorGeneration, SessionId,
};

pub const CONTROL_ENDPOINTS: [&str; 3] = [
    "https://cp.cloudflare.com/generate_204",
    "https://www.gstatic.com/generate_204",
    "https://www.msftconnecttest.com/connecttest.txt",
];

pub const CONTROL_QUORUM: usize = 2;
pub const MAX_CATEGORY_TARGETS: usize = 2;
pub const RESET_REQUIRED_FLOWS: u32 = 3;
pub const RESET_REQUIRED_TARGETS: u32 = 2;
pub const BLACKHOLE_REQUIRED_FLOWS: u32 = 2;
pub const BLACKHOLE_REQUIRED_TARGETS: u32 = 2;
pub const RESET_WINDOW: Duration = Duration::from_secs(30);
pub const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(5);
pub const GLOBAL_GATE_DEADLINE: Duration = Duration::from_secs(6);
pub const GATE_REPORT_TTL: Duration = Duration::from_secs(10);
pub const BASELINE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_BASELINE_SAMPLES_PER_KEY: usize = 9;
pub const MAX_BASELINE_KEYS: usize = 64;
pub const SLOW_BASELINE_MULTIPLIER: u64 = 3;
pub const SLOW_BASELINE_MARGIN_MS: u64 = 1_500;

const MILLIS_PER_SECOND: u64 = 1_000;

/// A complete, immutable fence for one gate observation.
///
/// The executor in later phases must compare this fence again before any
/// mutation. Phase 2 only carries and validates it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateFence {
    pub session_id: SessionId,
    pub lane_generation: LaneGeneration,
    pub sensor_generation: SensorGeneration,
    pub target_registry_version: RegistryVersion,
    pub network_fingerprint: NetworkFingerprint,
}

impl GateFence {
    pub fn from_envelope(
        envelope: EventEnvelope,
        lane_generation: LaneGeneration,
        network_fingerprint: NetworkFingerprint,
    ) -> Self {
        Self {
            session_id: envelope.session_id,
            lane_generation,
            sensor_generation: envelope.sensor_generation,
            target_registry_version: envelope.target_registry_version,
            network_fingerprint,
        }
    }
}

/// Caller-owned snapshot of the local network plane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalNetworkSnapshot {
    pub online: bool,
    pub interface_up: bool,
    pub default_route_available: bool,
    pub gateway_reachable: bool,
    pub network_fingerprint: NetworkFingerprint,
}

impl LocalNetworkSnapshot {
    pub fn is_locally_online(&self) -> bool {
        self.online && self.interface_up && self.default_route_available && self.gateway_reachable
    }
}

/// Eye state relevant to a single evidence window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub state: EyeHealthState,
    pub has_intersecting_gap: bool,
}

impl SensorSnapshot {
    pub const fn is_reliable(self) -> bool {
        matches!(self.state, EyeHealthState::Ready) && !self.has_intersecting_gap
    }
}

/// Already-deduplicated TLS evidence for one category and its current lane.
///
/// Assessment owns flow/target correlation. The gate receives only bounded
/// quorum counts from the current 30-second window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveEvidenceSummary {
    pub reset_after_client_hello_flows: u32,
    pub reset_targets: u32,
    pub confirmed_tls_blackhole_flows: u32,
    pub blackhole_targets: u32,
}

impl PassiveEvidenceSummary {
    pub const fn has_reset_quorum(self) -> bool {
        self.reset_after_client_hello_flows >= RESET_REQUIRED_FLOWS
            && self.reset_targets >= RESET_REQUIRED_TARGETS
    }

    pub const fn has_blackhole_quorum(self) -> bool {
        self.confirmed_tls_blackhole_flows >= BLACKHOLE_REQUIRED_FLOWS
            && self.blackhole_targets >= BLACKHOLE_REQUIRED_TARGETS
    }

    pub const fn has_blackhole_gate_quorum(self) -> bool {
        self.confirmed_tls_blackhole_flows >= BLACKHOLE_REQUIRED_FLOWS && self.blackhole_targets > 0
    }

    /// Any suspicious evidence prevents the current round from teaching its
    /// own latency baseline, even when it has not reached quorum yet.
    pub const fn is_suspect_round(self) -> bool {
        self.reset_after_client_hello_flows > 0 || self.confirmed_tls_blackhole_flows > 0
    }
}

/// The last phase reached by one endpoint probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProbeStage {
    UrlValidation,
    Dns,
    Transport,
    HttpResponse,
}

/// Result at [`EndpointProbeOutcome::stage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProbeResult {
    Succeeded,
    Failed,
    TimedOut,
}

/// In-memory network result. `endpoint` may contain an observed SNI and must be
/// reduced to typed counts before any local logging or UI projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointProbeOutcome {
    pub endpoint: String,
    pub stage: EndpointProbeStage,
    pub result: EndpointProbeResult,
    pub duration_ms: u64,
    pub http_status: Option<u16>,
}

impl EndpointProbeOutcome {
    pub fn http(endpoint: impl Into<String>, duration_ms: u64, status: u16) -> Self {
        Self {
            endpoint: endpoint.into(),
            stage: EndpointProbeStage::HttpResponse,
            result: EndpointProbeResult::Succeeded,
            duration_ms,
            http_status: Some(status),
        }
    }

    pub fn failed(
        endpoint: impl Into<String>,
        stage: EndpointProbeStage,
        duration_ms: u64,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            stage,
            result: EndpointProbeResult::Failed,
            duration_ms,
            http_status: None,
        }
    }

    pub fn timed_out(
        endpoint: impl Into<String>,
        stage: EndpointProbeStage,
        duration_ms: u64,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            stage,
            result: EndpointProbeResult::TimedOut,
            duration_ms,
            http_status: None,
        }
    }

    pub const fn dns_succeeded(&self) -> bool {
        match self.stage {
            EndpointProbeStage::UrlValidation | EndpointProbeStage::Dns => {
                matches!(self.result, EndpointProbeResult::Succeeded)
            }
            EndpointProbeStage::Transport | EndpointProbeStage::HttpResponse => true,
        }
    }

    pub const fn has_http_response(&self) -> bool {
        matches!(self.stage, EndpointProbeStage::HttpResponse)
            && matches!(self.result, EndpointProbeResult::Succeeded)
            && self.http_status.is_some()
    }

    /// HTTP 4xx proves target reachability. HTTP 5xx is an upstream target
    /// failure and must never be promoted to DPI evidence.
    pub const fn category_target_reachable(&self) -> bool {
        self.has_http_response() && matches!(self.http_status, Some(status) if status < 500)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateClassification {
    Stable,
    Offline,
    DnsFailure,
    UpstreamDegraded,
    TargetUnavailable,
    ServiceSlow,
    DpiSuspected,
    DpiBlocked,
    SensorUnreliable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateRequestError {
    EmptyCategory,
    NoCategoryTargets,
    TooManyCategoryTargets { actual: usize, maximum: usize },
    NetworkFingerprintMismatch,
}

impl fmt::Display for GateRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCategory => formatter.write_str("gate category must not be empty"),
            Self::NoCategoryTargets => {
                formatter.write_str("gate requires at least one category target")
            }
            Self::TooManyCategoryTargets { actual, maximum } => write!(
                formatter,
                "gate received {actual} category targets; the maximum is {maximum}"
            ),
            Self::NetworkFingerprintMismatch => formatter
                .write_str("gate fence and local snapshot contain different network fingerprints"),
        }
    }
}

impl std::error::Error for GateRequestError {}

/// Immutable request assembled by Manager after assessment fencing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRequest {
    pub fence: GateFence,
    pub category: String,
    pub category_targets: Vec<String>,
    pub local_network: LocalNetworkSnapshot,
    pub sensor: SensorSnapshot,
    pub passive_evidence: PassiveEvidenceSummary,
    pub requested_at_monotonic_ms: u64,
}

impl GateRequest {
    pub fn validate(&self) -> Result<(), GateRequestError> {
        if self.category.trim().is_empty() {
            return Err(GateRequestError::EmptyCategory);
        }
        if self.category_targets.is_empty() {
            return Err(GateRequestError::NoCategoryTargets);
        }
        if self.category_targets.len() > MAX_CATEGORY_TARGETS {
            return Err(GateRequestError::TooManyCategoryTargets {
                actual: self.category_targets.len(),
                maximum: MAX_CATEGORY_TARGETS,
            });
        }
        if self.fence.network_fingerprint != self.local_network.network_fingerprint {
            return Err(GateRequestError::NetworkFingerprintMismatch);
        }
        Ok(())
    }
}

/// A fresh, fenced observation. The report remains diagnostic after expiry,
/// but it is no longer eligible to authorize an intent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateReport {
    pub fence: GateFence,
    pub category: String,
    pub classification: GateClassification,
    pub controls: Vec<EndpointProbeOutcome>,
    pub category_targets: Vec<EndpointProbeOutcome>,
    pub baseline_latency_ms: Option<u64>,
    pub slow_threshold_ms: Option<u64>,
    pub generated_at_monotonic_ms: u64,
    pub valid_until_monotonic_ms: u64,
}

impl GateReport {
    pub fn is_fresh(&self, now_monotonic_ms: u64, expected_fence: &GateFence) -> bool {
        &self.fence == expected_fence
            && now_monotonic_ms >= self.generated_at_monotonic_ms
            && now_monotonic_ms <= self.valid_until_monotonic_ms
    }
}

pub const fn slow_threshold_ms(baseline_ms: u64) -> u64 {
    let multiplied = baseline_ms.saturating_mul(SLOW_BASELINE_MULTIPLIER);
    let margin = baseline_ms.saturating_add(SLOW_BASELINE_MARGIN_MS);
    if multiplied > margin {
        multiplied
    } else {
        margin
    }
}

/// Pure precedence-ordered gate classification.
///
/// `baseline_latency_ms` must have been read before this round. A suspect
/// round cannot become actionable by seeding itself.
pub fn classify_gate(
    local_network: &LocalNetworkSnapshot,
    sensor: SensorSnapshot,
    passive_evidence: PassiveEvidenceSummary,
    controls: &[EndpointProbeOutcome],
    category_targets: &[EndpointProbeOutcome],
    baseline_latency_ms: Option<u64>,
) -> GateClassification {
    if !sensor.is_reliable() {
        return GateClassification::SensorUnreliable;
    }

    if !local_network.is_locally_online() {
        return GateClassification::Offline;
    }

    let control_dns_quorum = controls
        .iter()
        .filter(|outcome| outcome.dns_succeeded())
        .count();
    if control_dns_quorum < CONTROL_QUORUM {
        return GateClassification::DnsFailure;
    }

    let control_http_quorum = controls
        .iter()
        .filter(|outcome| outcome.has_http_response())
        .count();
    if control_http_quorum < CONTROL_QUORUM {
        return GateClassification::UpstreamDegraded;
    }

    if category_targets.is_empty()
        || category_targets.iter().any(|outcome| {
            outcome.has_http_response()
                && matches!(outcome.http_status, Some(status) if status >= 500)
        })
    {
        return GateClassification::TargetUnavailable;
    }

    let reachable_target_count = category_targets
        .iter()
        .filter(|outcome| outcome.category_target_reachable())
        .count();
    // One failed target next to a reachable peer is a site-specific failure,
    // not DPI. A single exact incident target may proceed only when passive
    // Eyes independently confirmed repeated TLS blackholes and the fresh probe
    // reached DNS before failing. It remains medium-confidence DpiSuspected;
    // candidate confirmation and exact rollback decide whether mutation sticks.
    let confirmed_single_target_blackhole = category_targets.len() == 1
        && category_targets[0].dns_succeeded()
        && passive_evidence.has_blackhole_gate_quorum();
    if (reachable_target_count > 0 && reachable_target_count < category_targets.len())
        || (reachable_target_count == 0
            && category_targets.len() == 1
            && !confirmed_single_target_blackhole)
    {
        return GateClassification::TargetUnavailable;
    }

    // Stable identity is mandatory both for a latency comparison and for an
    // actionable DPI verdict. Unknown/unstable networks stay diagnostic-only.
    let trusted_baseline = local_network
        .network_fingerprint
        .is_stable()
        .then_some(baseline_latency_ms)
        .flatten();

    if let Some(baseline_ms) = trusted_baseline {
        let threshold_ms = slow_threshold_ms(baseline_ms);
        if category_targets
            .iter()
            .filter(|outcome| outcome.category_target_reachable())
            .any(|outcome| outcome.duration_ms >= threshold_ms)
        {
            return GateClassification::ServiceSlow;
        }

        if passive_evidence.has_blackhole_quorum() {
            return GateClassification::DpiBlocked;
        }

        if passive_evidence.has_reset_quorum() {
            return GateClassification::DpiSuspected;
        }

        if confirmed_single_target_blackhole {
            return GateClassification::DpiSuspected;
        }
    }

    // A current passive blackhole cannot become Stable merely because one
    // active request got through. Without a trusted latency baseline the Gate
    // remains diagnostic-only and policy rejects mutation.
    if confirmed_single_target_blackhole {
        return GateClassification::TargetUnavailable;
    }

    if reachable_target_count == 0 {
        return GateClassification::TargetUnavailable;
    }

    GateClassification::Stable
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BaselineSample {
    measured_at_monotonic_ms: u64,
    duration_ms: u64,
}

#[derive(Clone, Debug, Default)]
struct BaselineEntry {
    samples: Vec<BaselineSample>,
    last_updated_monotonic_ms: u64,
}

/// Process-local rolling latency memory. It is intentionally not persisted.
#[derive(Clone, Debug, Default)]
pub struct LatencyBaselineStore {
    entries: BTreeMap<String, BaselineEntry>,
}

impl LatencyBaselineStore {
    /// Returns the rolling median for a stable network after pruning its TTL.
    pub fn median_ms(
        &mut self,
        fingerprint: &NetworkFingerprint,
        now_monotonic_ms: u64,
    ) -> Option<u64> {
        self.prune(now_monotonic_ms);
        let key = fingerprint.stable_key()?;
        let entry = self.entries.get(key)?;
        median(entry.samples.iter().map(|sample| sample.duration_ms))
    }

    /// Records successful control latencies. Suspect rounds are rejected as a
    /// whole so they cannot manufacture the baseline used to classify them.
    pub fn record_control_round(
        &mut self,
        fingerprint: &NetworkFingerprint,
        successful_control_latencies_ms: impl IntoIterator<Item = u64>,
        measured_at_monotonic_ms: u64,
        suspect_round: bool,
    ) -> usize {
        self.prune(measured_at_monotonic_ms);
        if suspect_round {
            return 0;
        }

        let Some(key) = fingerprint.stable_key() else {
            return 0;
        };

        let samples: Vec<_> = successful_control_latencies_ms
            .into_iter()
            .map(|duration_ms| BaselineSample {
                measured_at_monotonic_ms,
                duration_ms,
            })
            .collect();
        if samples.is_empty() {
            return 0;
        }

        if !self.entries.contains_key(key) && self.entries.len() >= MAX_BASELINE_KEYS {
            self.evict_oldest_entry();
        }

        let entry = self.entries.entry(key.to_owned()).or_default();
        let inserted = samples.len();
        entry.samples.extend(samples);
        if entry.samples.len() > MAX_BASELINE_SAMPLES_PER_KEY {
            let overflow = entry.samples.len() - MAX_BASELINE_SAMPLES_PER_KEY;
            entry.samples.drain(..overflow);
        }
        entry.last_updated_monotonic_ms = measured_at_monotonic_ms;
        inserted
    }

    pub fn sample_count(&mut self, now_monotonic_ms: u64) -> usize {
        self.prune(now_monotonic_ms);
        self.entries.values().map(|entry| entry.samples.len()).sum()
    }

    pub fn key_count(&mut self, now_monotonic_ms: u64) -> usize {
        self.prune(now_monotonic_ms);
        self.entries.len()
    }

    fn prune(&mut self, now_monotonic_ms: u64) {
        let ttl_ms = duration_ms(BASELINE_TTL);
        self.entries.retain(|_, entry| {
            entry.samples.retain(|sample| {
                sample.measured_at_monotonic_ms <= now_monotonic_ms
                    && now_monotonic_ms.saturating_sub(sample.measured_at_monotonic_ms) <= ttl_ms
            });
            !entry.samples.is_empty()
        });
    }

    fn evict_oldest_entry(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(key, entry)| (entry.last_updated_monotonic_ms, *key))
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

fn median(values: impl IntoIterator<Item = u64>) -> Option<u64> {
    let mut values: Vec<_> = values.into_iter().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[middle])
    } else {
        let lower = values[middle - 1];
        let upper = values[middle];
        Some(lower.saturating_add(upper.saturating_sub(lower) / 2))
    }
}

pub type EndpointProbeFuture<'a> = Pin<Box<dyn Future<Output = EndpointProbeOutcome> + Send + 'a>>;

/// Injectable boundary for production HTTP/DNS I/O.
pub trait EndpointProbeBackend: Send + Sync {
    fn probe(&self, endpoint: String, timeout: Duration) -> EndpointProbeFuture<'_>;
}

#[derive(Clone, Debug)]
pub struct ReqwestProbeBackend {
    client: reqwest::Client,
}

impl ReqwestProbeBackend {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .connect_timeout(ENDPOINT_TIMEOUT)
            .timeout(ENDPOINT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { client })
    }

    async fn probe_endpoint(&self, endpoint: String, timeout: Duration) -> EndpointProbeOutcome {
        let started = Instant::now();
        let url = match normalized_https_url(&endpoint) {
            Ok(url) => url,
            Err(()) => {
                return EndpointProbeOutcome::failed(
                    endpoint,
                    EndpointProbeStage::UrlValidation,
                    elapsed_ms(started),
                );
            }
        };

        let host = url.host_str().expect("validated HTTPS URL has a host");
        let port = url.port_or_known_default().unwrap_or(443);
        let Some(dns_budget) = timeout.checked_sub(started.elapsed()) else {
            return EndpointProbeOutcome::timed_out(
                endpoint,
                EndpointProbeStage::Dns,
                elapsed_ms(started),
            );
        };

        match tokio::time::timeout(dns_budget, lookup_host((host, port))).await {
            Ok(Ok(mut addresses)) => {
                if addresses.next().is_some() {
                    // At least one address proves DNS reachability. Reqwest
                    // still performs its own connection-aware resolution.
                } else {
                    return EndpointProbeOutcome::failed(
                        endpoint,
                        EndpointProbeStage::Dns,
                        elapsed_ms(started),
                    );
                }
            }
            Ok(Err(_)) => {
                return EndpointProbeOutcome::failed(
                    endpoint,
                    EndpointProbeStage::Dns,
                    elapsed_ms(started),
                );
            }
            Err(_) => {
                return EndpointProbeOutcome::timed_out(
                    endpoint,
                    EndpointProbeStage::Dns,
                    elapsed_ms(started),
                );
            }
        }

        let Some(request_budget) = timeout.checked_sub(started.elapsed()) else {
            return EndpointProbeOutcome::timed_out(
                endpoint,
                EndpointProbeStage::Transport,
                elapsed_ms(started),
            );
        };

        match self.client.get(url).timeout(request_budget).send().await {
            Ok(response) => EndpointProbeOutcome::http(
                endpoint,
                elapsed_ms(started),
                response.status().as_u16(),
            ),
            Err(error) if error.is_timeout() => EndpointProbeOutcome::timed_out(
                endpoint,
                EndpointProbeStage::Transport,
                elapsed_ms(started),
            ),
            Err(_) => EndpointProbeOutcome::failed(
                endpoint,
                EndpointProbeStage::Transport,
                elapsed_ms(started),
            ),
        }
    }
}

impl EndpointProbeBackend for ReqwestProbeBackend {
    fn probe(&self, endpoint: String, timeout: Duration) -> EndpointProbeFuture<'_> {
        Box::pin(self.probe_endpoint(endpoint, timeout))
    }
}

/// Gate runtime with a bounded process-local baseline and injectable probes.
pub struct EnvironmentGate<B = ReqwestProbeBackend> {
    backend: Arc<B>,
    baselines: LatencyBaselineStore,
}

impl EnvironmentGate<ReqwestProbeBackend> {
    pub fn production() -> Result<Self, reqwest::Error> {
        Ok(Self::with_backend(ReqwestProbeBackend::new()?))
    }
}

impl<B> EnvironmentGate<B>
where
    B: EndpointProbeBackend + 'static,
{
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            baselines: LatencyBaselineStore::default(),
        }
    }

    pub fn with_backend_and_baselines(backend: B, baselines: LatencyBaselineStore) -> Self {
        Self {
            backend: Arc::new(backend),
            baselines,
        }
    }

    pub fn baselines_mut(&mut self) -> &mut LatencyBaselineStore {
        &mut self.baselines
    }

    pub async fn evaluate(&mut self, request: GateRequest) -> Result<GateReport, GateRequestError> {
        let baseline_clock_ms = request.requested_at_monotonic_ms;
        self.evaluate_with_baseline_clock(request, baseline_clock_ms)
            .await
    }

    /// Production observer restarts reset Eyes/Manager timestamps. Baseline
    /// retention uses the process-wide recovery clock supplied here, while the
    /// returned report keeps the Manager-local clock used by evidence fences.
    pub async fn evaluate_with_baseline_clock(
        &mut self,
        request: GateRequest,
        baseline_clock_ms: u64,
    ) -> Result<GateReport, GateRequestError> {
        request.validate()?;
        let started = Instant::now();
        let preexisting_baseline = self.baselines.median_ms(
            &request.local_network.network_fingerprint,
            baseline_clock_ms,
        );

        let (controls, category_targets) = probe_round(
            Arc::clone(&self.backend),
            request.category_targets.as_slice(),
        )
        .await;
        let generated_at_monotonic_ms = request
            .requested_at_monotonic_ms
            .saturating_add(elapsed_ms(started));

        let classification = classify_gate(
            &request.local_network,
            request.sensor,
            request.passive_evidence,
            &controls,
            &category_targets,
            preexisting_baseline,
        );

        let successful_control_latencies = controls
            .iter()
            .filter(|outcome| outcome.has_http_response())
            .map(|outcome| outcome.duration_ms)
            .collect::<Vec<_>>();
        if successful_control_latencies.len() >= CONTROL_QUORUM
            && request.local_network.is_locally_online()
            && request.sensor.is_reliable()
        {
            self.baselines.record_control_round(
                &request.local_network.network_fingerprint,
                successful_control_latencies,
                baseline_clock_ms.saturating_add(elapsed_ms(started)),
                request.passive_evidence.is_suspect_round(),
            );
        }

        Ok(GateReport {
            fence: request.fence,
            category: request.category,
            classification,
            controls,
            category_targets,
            baseline_latency_ms: preexisting_baseline,
            slow_threshold_ms: preexisting_baseline.map(slow_threshold_ms),
            generated_at_monotonic_ms,
            valid_until_monotonic_ms: generated_at_monotonic_ms
                .saturating_add(duration_ms(GATE_REPORT_TTL)),
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum ProbeSlot {
    Control(usize),
    Category(usize),
}

async fn probe_round<B>(
    backend: Arc<B>,
    category_targets: &[String],
) -> (Vec<EndpointProbeOutcome>, Vec<EndpointProbeOutcome>)
where
    B: EndpointProbeBackend + 'static,
{
    probe_round_with_deadline(
        backend,
        category_targets,
        ENDPOINT_TIMEOUT,
        GLOBAL_GATE_DEADLINE,
    )
    .await
}

async fn probe_round_with_deadline<B>(
    backend: Arc<B>,
    category_targets: &[String],
    endpoint_timeout: Duration,
    global_deadline: Duration,
) -> (Vec<EndpointProbeOutcome>, Vec<EndpointProbeOutcome>)
where
    B: EndpointProbeBackend + 'static,
{
    let mut tasks = JoinSet::new();

    for (index, endpoint) in CONTROL_ENDPOINTS.iter().enumerate() {
        spawn_probe(
            &mut tasks,
            Arc::clone(&backend),
            ProbeSlot::Control(index),
            (*endpoint).to_owned(),
            endpoint_timeout,
        );
    }
    for (index, endpoint) in category_targets.iter().enumerate() {
        spawn_probe(
            &mut tasks,
            Arc::clone(&backend),
            ProbeSlot::Category(index),
            endpoint.clone(),
            endpoint_timeout,
        );
    }

    let timeout_ms = duration_ms(global_deadline);
    let mut controls: Vec<Option<EndpointProbeOutcome>> = CONTROL_ENDPOINTS
        .iter()
        .map(|endpoint| {
            Some(EndpointProbeOutcome::timed_out(
                *endpoint,
                EndpointProbeStage::Transport,
                timeout_ms,
            ))
        })
        .collect();
    let mut categories: Vec<Option<EndpointProbeOutcome>> = category_targets
        .iter()
        .map(|endpoint| {
            Some(EndpointProbeOutcome::timed_out(
                endpoint,
                EndpointProbeStage::Transport,
                timeout_ms,
            ))
        })
        .collect();

    let deadline = tokio::time::Instant::now() + global_deadline;
    loop {
        let joined = match tokio::time::timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(joined)) => joined,
            Ok(None) | Err(_) => break,
        };
        let Ok((slot, outcome)) = joined else {
            continue;
        };
        match slot {
            ProbeSlot::Control(index) => controls[index] = Some(outcome),
            ProbeSlot::Category(index) => categories[index] = Some(outcome),
        }
    }
    tasks.abort_all();

    // Slots start with transport-level timeouts. The production backend owns
    // the endpoint budget and reports the last stage it actually reached;
    // racing it with an equal outer timeout would turn a transport/TLS hang
    // into a false DNS failure. The longer global deadline remains an
    // emergency fence for a panicked or non-cooperative backend.
    (
        controls.into_iter().flatten().collect(),
        categories.into_iter().flatten().collect(),
    )
}

fn spawn_probe<B>(
    tasks: &mut JoinSet<(ProbeSlot, EndpointProbeOutcome)>,
    backend: Arc<B>,
    slot: ProbeSlot,
    endpoint: String,
    endpoint_timeout: Duration,
) where
    B: EndpointProbeBackend + 'static,
{
    tasks.spawn(async move {
        let outcome = backend.probe(endpoint, endpoint_timeout).await;
        (slot, outcome)
    });
}

fn normalized_https_url(endpoint: &str) -> Result<reqwest::Url, ()> {
    let candidate = if endpoint.contains("://") {
        endpoint.to_owned()
    } else {
        format!("https://{endpoint}/")
    };
    let url = reqwest::Url::parse(&candidate).map_err(|_| ())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(());
    }
    Ok(url)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

const fn duration_ms(duration: Duration) -> u64 {
    duration.as_secs().saturating_mul(MILLIS_PER_SECOND) + duration.subsec_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tokio::sync::Barrier;

    use super::*;

    const NOW: u64 = 1_000_000;

    fn stable_fingerprint() -> NetworkFingerprint {
        NetworkFingerprint::Stable {
            key: "gateway:test-network".to_owned(),
        }
    }

    fn local_network() -> LocalNetworkSnapshot {
        LocalNetworkSnapshot {
            online: true,
            interface_up: true,
            default_route_available: true,
            gateway_reachable: true,
            network_fingerprint: stable_fingerprint(),
        }
    }

    fn ready_sensor() -> SensorSnapshot {
        SensorSnapshot {
            state: EyeHealthState::Ready,
            has_intersecting_gap: false,
        }
    }

    fn healthy_controls() -> Vec<EndpointProbeOutcome> {
        vec![
            EndpointProbeOutcome::http(CONTROL_ENDPOINTS[0], 100, 204),
            EndpointProbeOutcome::http(CONTROL_ENDPOINTS[1], 120, 204),
            EndpointProbeOutcome::http(CONTROL_ENDPOINTS[2], 140, 200),
        ]
    }

    fn reachable_targets() -> Vec<EndpointProbeOutcome> {
        vec![
            EndpointProbeOutcome::http("one.example", 200, 200),
            EndpointProbeOutcome::http("two.example", 250, 404),
        ]
    }

    fn classify(
        local: &LocalNetworkSnapshot,
        sensor: SensorSnapshot,
        passive: PassiveEvidenceSummary,
        controls: &[EndpointProbeOutcome],
        targets: &[EndpointProbeOutcome],
        baseline: Option<u64>,
    ) -> GateClassification {
        classify_gate(local, sensor, passive, controls, targets, baseline)
    }

    #[test]
    fn policy_constants_match_the_approved_design() {
        assert_eq!(CONTROL_QUORUM, 2);
        assert_eq!(RESET_REQUIRED_FLOWS, 3);
        assert_eq!(RESET_REQUIRED_TARGETS, 2);
        assert_eq!(BLACKHOLE_REQUIRED_FLOWS, 2);
        assert_eq!(BLACKHOLE_REQUIRED_TARGETS, 2);
        assert_eq!(RESET_WINDOW, Duration::from_secs(30));
        assert_eq!(ENDPOINT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(GLOBAL_GATE_DEADLINE, Duration::from_secs(6));
        assert_eq!(GATE_REPORT_TTL, Duration::from_secs(10));
        assert_eq!(BASELINE_TTL, Duration::from_secs(24 * 60 * 60));
    }

    #[test]
    fn sensor_unreliable_has_highest_precedence() {
        let mut offline = local_network();
        offline.online = false;
        for sensor in [
            SensorSnapshot {
                state: EyeHealthState::Degraded,
                has_intersecting_gap: false,
            },
            SensorSnapshot {
                state: EyeHealthState::Blind,
                has_intersecting_gap: false,
            },
            SensorSnapshot {
                state: EyeHealthState::Stopped,
                has_intersecting_gap: false,
            },
            SensorSnapshot {
                state: EyeHealthState::Ready,
                has_intersecting_gap: true,
            },
        ] {
            assert_eq!(
                classify(
                    &offline,
                    sensor,
                    PassiveEvidenceSummary::default(),
                    &[],
                    &[],
                    None
                ),
                GateClassification::SensorUnreliable
            );
        }
    }

    #[test]
    fn every_local_plane_failure_is_offline() {
        for mutate in [
            |snapshot: &mut LocalNetworkSnapshot| snapshot.online = false,
            |snapshot: &mut LocalNetworkSnapshot| snapshot.interface_up = false,
            |snapshot: &mut LocalNetworkSnapshot| snapshot.default_route_available = false,
            |snapshot: &mut LocalNetworkSnapshot| snapshot.gateway_reachable = false,
        ] {
            let mut local = local_network();
            mutate(&mut local);
            assert_eq!(
                classify(
                    &local,
                    ready_sensor(),
                    PassiveEvidenceSummary::default(),
                    &[],
                    &[],
                    None,
                ),
                GateClassification::Offline
            );
        }
    }

    #[test]
    fn dns_requires_two_of_three_controls() {
        let controls = vec![
            EndpointProbeOutcome::http("one", 10, 204),
            EndpointProbeOutcome::failed("two", EndpointProbeStage::Dns, 10),
            EndpointProbeOutcome::timed_out("three", EndpointProbeStage::Dns, 10),
        ];

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                PassiveEvidenceSummary::default(),
                &controls,
                &reachable_targets(),
                Some(100),
            ),
            GateClassification::DnsFailure
        );
    }

    #[test]
    fn resolved_controls_without_http_quorum_are_upstream_degraded() {
        let controls = vec![
            EndpointProbeOutcome::http("one", 10, 503),
            EndpointProbeOutcome::failed("two", EndpointProbeStage::Transport, 20),
            EndpointProbeOutcome::timed_out("three", EndpointProbeStage::Transport, 30),
        ];

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                PassiveEvidenceSummary::default(),
                &controls,
                &reachable_targets(),
                Some(100),
            ),
            GateClassification::UpstreamDegraded
        );
    }

    #[test]
    fn control_http_5xx_still_proves_internet_reachability() {
        let controls = vec![
            EndpointProbeOutcome::http("one", 10, 500),
            EndpointProbeOutcome::http("two", 20, 503),
            EndpointProbeOutcome::failed("three", EndpointProbeStage::Dns, 5),
        ];

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                PassiveEvidenceSummary::default(),
                &controls,
                &reachable_targets(),
                Some(100),
            ),
            GateClassification::Stable
        );
    }

    #[test]
    fn category_failure_and_http_5xx_are_target_unavailable_but_4xx_is_reachable() {
        let controls = healthy_controls();
        let passive = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            ..PassiveEvidenceSummary::default()
        };

        for failed in [
            EndpointProbeOutcome::failed("target", EndpointProbeStage::Transport, 20),
            EndpointProbeOutcome::http("target", 20, 500),
        ] {
            assert_eq!(
                classify(
                    &local_network(),
                    ready_sensor(),
                    passive,
                    &controls,
                    &[failed],
                    Some(100),
                ),
                GateClassification::TargetUnavailable
            );
        }

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                PassiveEvidenceSummary::default(),
                &controls,
                &[EndpointProbeOutcome::http("target", 20, 403)],
                Some(100),
            ),
            GateClassification::Stable
        );
    }

    #[test]
    fn confirmed_single_target_tls_blackhole_is_dpi_suspected() {
        let passive = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 1,
            ..PassiveEvidenceSummary::default()
        };
        let target =
            EndpointProbeOutcome::timed_out("video.example", EndpointProbeStage::Transport, 5_000);

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                passive,
                &healthy_controls(),
                &[target],
                Some(100),
            ),
            GateClassification::DpiSuspected
        );
    }

    #[test]
    fn reachable_exact_target_cannot_refute_current_passive_blackholes() {
        let passive = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 1,
            ..PassiveEvidenceSummary::default()
        };

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                passive,
                &healthy_controls(),
                &[EndpointProbeOutcome::http("video.example", 20, 200)],
                Some(100),
            ),
            GateClassification::DpiSuspected
        );
    }

    #[test]
    fn single_target_blackhole_without_a_trusted_baseline_is_not_stable() {
        let passive = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 1,
            ..PassiveEvidenceSummary::default()
        };

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                passive,
                &healthy_controls(),
                &[EndpointProbeOutcome::http("video.example", 20, 200)],
                None,
            ),
            GateClassification::TargetUnavailable
        );
    }

    #[test]
    fn single_target_blackhole_never_overrides_dns_5xx_or_slow_service() {
        let passive = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 1,
            ..PassiveEvidenceSummary::default()
        };
        for target in [
            EndpointProbeOutcome::failed("video.example", EndpointProbeStage::Dns, 20),
            EndpointProbeOutcome::http("video.example", 20, 500),
        ] {
            assert_eq!(
                classify(
                    &local_network(),
                    ready_sensor(),
                    passive,
                    &healthy_controls(),
                    &[target],
                    Some(100),
                ),
                GateClassification::TargetUnavailable
            );
        }

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                passive,
                &healthy_controls(),
                &[EndpointProbeOutcome::http("video.example", 2_000, 200)],
                Some(500),
            ),
            GateClassification::ServiceSlow
        );
    }

    #[test]
    fn shared_transport_failure_with_independent_blackholes_is_dpi_blocked() {
        let targets = vec![
            EndpointProbeOutcome::failed("one", EndpointProbeStage::Transport, 5_000),
            EndpointProbeOutcome::timed_out("two", EndpointProbeStage::Transport, 5_000),
        ];
        let blackhole = PassiveEvidenceSummary {
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 2,
            ..PassiveEvidenceSummary::default()
        };
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                blackhole,
                &healthy_controls(),
                &targets,
                Some(100),
            ),
            GateClassification::DpiBlocked
        );
    }

    #[test]
    fn service_slow_requires_preexisting_baseline_and_precedes_dpi() {
        let controls = healthy_controls();
        let slow = vec![EndpointProbeOutcome::http("target", 2_000, 200)];
        let reset = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            ..PassiveEvidenceSummary::default()
        };

        assert_eq!(slow_threshold_ms(500), 2_000);
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                reset,
                &controls,
                &slow,
                Some(500),
            ),
            GateClassification::ServiceSlow
        );
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                reset,
                &controls,
                &slow,
                None,
            ),
            GateClassification::Stable
        );
    }

    #[test]
    fn blackhole_and_reset_quorums_are_exact_and_blackhole_wins() {
        let controls = healthy_controls();
        let targets = reachable_targets();
        let both = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            confirmed_tls_blackhole_flows: 2,
            blackhole_targets: 2,
        };
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                both,
                &controls,
                &targets,
                Some(100),
            ),
            GateClassification::DpiBlocked
        );

        let reset_only = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            ..PassiveEvidenceSummary::default()
        };
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                reset_only,
                &controls,
                &targets,
                Some(100),
            ),
            GateClassification::DpiSuspected
        );

        for below in [
            PassiveEvidenceSummary {
                reset_after_client_hello_flows: 2,
                reset_targets: 2,
                ..PassiveEvidenceSummary::default()
            },
            PassiveEvidenceSummary {
                reset_after_client_hello_flows: 3,
                reset_targets: 1,
                ..PassiveEvidenceSummary::default()
            },
            PassiveEvidenceSummary {
                confirmed_tls_blackhole_flows: 1,
                blackhole_targets: 2,
                ..PassiveEvidenceSummary::default()
            },
            PassiveEvidenceSummary {
                confirmed_tls_blackhole_flows: 2,
                blackhole_targets: 1,
                ..PassiveEvidenceSummary::default()
            },
        ] {
            assert_eq!(
                classify(
                    &local_network(),
                    ready_sensor(),
                    below,
                    &controls,
                    &targets,
                    Some(100),
                ),
                GateClassification::Stable
            );
        }
    }

    #[test]
    fn dpi_needs_preexisting_baseline_and_stable_network_identity() {
        let reset = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            ..PassiveEvidenceSummary::default()
        };
        let controls = healthy_controls();
        let targets = reachable_targets();

        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                reset,
                &controls,
                &targets,
                None,
            ),
            GateClassification::Stable
        );

        let mut unknown = local_network();
        unknown.network_fingerprint = NetworkFingerprint::Unknown;
        assert_eq!(
            classify(
                &unknown,
                ready_sensor(),
                reset,
                &controls,
                &targets,
                Some(100),
            ),
            GateClassification::Stable
        );
    }

    #[test]
    fn baseline_is_median_bounded_and_expires_after_24_hours() {
        let fingerprint = stable_fingerprint();
        let mut store = LatencyBaselineStore::default();
        assert_eq!(
            store.record_control_round(&fingerprint, 1_u64..=12, NOW, false),
            12
        );
        assert_eq!(store.sample_count(NOW), MAX_BASELINE_SAMPLES_PER_KEY);
        // The newest nine values are 4..=12; their median is 8.
        assert_eq!(store.median_ms(&fingerprint, NOW), Some(8));

        let still_valid = NOW + duration_ms(BASELINE_TTL);
        assert_eq!(store.median_ms(&fingerprint, still_valid), Some(8));
        assert_eq!(store.median_ms(&fingerprint, still_valid + 1), None);
    }

    #[test]
    fn suspect_or_unstable_round_never_seeds_baseline() {
        let fingerprint = stable_fingerprint();
        let mut store = LatencyBaselineStore::default();
        assert_eq!(
            store.record_control_round(&fingerprint, [100, 110, 120], NOW, true),
            0
        );
        assert_eq!(
            store.record_control_round(&NetworkFingerprint::Unknown, [100, 110], NOW, false),
            0
        );
        assert_eq!(store.sample_count(NOW), 0);
    }

    #[test]
    fn baseline_key_count_is_bounded_and_oldest_is_evicted() {
        let mut store = LatencyBaselineStore::default();
        for index in 0..=MAX_BASELINE_KEYS {
            let fingerprint = NetworkFingerprint::Stable {
                key: format!("network-{index:03}"),
            };
            store.record_control_round(&fingerprint, [index as u64], NOW + index as u64, false);
        }
        assert_eq!(
            store.key_count(NOW + MAX_BASELINE_KEYS as u64),
            MAX_BASELINE_KEYS
        );
        assert_eq!(
            store.median_ms(
                &NetworkFingerprint::Stable {
                    key: "network-000".to_owned(),
                },
                NOW + MAX_BASELINE_KEYS as u64,
            ),
            None
        );
    }

    #[test]
    fn report_ttl_and_full_fence_are_checked() {
        let fence = GateFence {
            session_id: SessionId(1),
            lane_generation: LaneGeneration(2),
            sensor_generation: SensorGeneration(3),
            target_registry_version: RegistryVersion(4),
            network_fingerprint: stable_fingerprint(),
        };
        let report = GateReport {
            fence: fence.clone(),
            category: "youtube".to_owned(),
            classification: GateClassification::Stable,
            controls: vec![],
            category_targets: vec![],
            baseline_latency_ms: None,
            slow_threshold_ms: None,
            generated_at_monotonic_ms: NOW,
            valid_until_monotonic_ms: NOW + duration_ms(GATE_REPORT_TTL),
        };

        assert!(report.is_fresh(NOW, &fence));
        assert!(report.is_fresh(NOW + duration_ms(GATE_REPORT_TTL), &fence));
        assert!(!report.is_fresh(NOW - 1, &fence));
        assert!(!report.is_fresh(NOW + duration_ms(GATE_REPORT_TTL) + 1, &fence));

        let mut stale_fence = fence;
        stale_fence.lane_generation = LaneGeneration(99);
        assert!(!report.is_fresh(NOW, &stale_fence));
    }

    #[derive(Clone, Debug)]
    struct FakeBackend {
        outcomes: Arc<BTreeMap<String, EndpointProbeOutcome>>,
    }

    impl EndpointProbeBackend for FakeBackend {
        fn probe(&self, endpoint: String, _timeout: Duration) -> EndpointProbeFuture<'_> {
            Box::pin(async move {
                self.outcomes.get(&endpoint).cloned().unwrap_or_else(|| {
                    EndpointProbeOutcome::failed(endpoint, EndpointProbeStage::Dns, 1)
                })
            })
        }
    }

    fn gate_request(passive_evidence: PassiveEvidenceSummary, now: u64) -> GateRequest {
        let fingerprint = stable_fingerprint();
        GateRequest {
            fence: GateFence {
                session_id: SessionId(1),
                lane_generation: LaneGeneration(2),
                sensor_generation: SensorGeneration(3),
                target_registry_version: RegistryVersion(4),
                network_fingerprint: fingerprint.clone(),
            },
            category: "youtube".to_owned(),
            category_targets: vec!["video.example".to_owned()],
            local_network: LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint: fingerprint,
            },
            sensor: ready_sensor(),
            passive_evidence,
            requested_at_monotonic_ms: now,
        }
    }

    fn fake_backend() -> FakeBackend {
        let mut outcomes = BTreeMap::new();
        for (index, endpoint) in CONTROL_ENDPOINTS.iter().enumerate() {
            outcomes.insert(
                (*endpoint).to_owned(),
                EndpointProbeOutcome::http(*endpoint, 100 + index as u64 * 10, 204),
            );
        }
        outcomes.insert(
            "video.example".to_owned(),
            EndpointProbeOutcome::http("video.example", 200, 200),
        );
        FakeBackend {
            outcomes: Arc::new(outcomes),
        }
    }

    #[tokio::test]
    async fn clean_round_seeds_only_the_next_round() {
        let mut gate = EnvironmentGate::with_backend(fake_backend());
        let first = gate
            .evaluate(gate_request(PassiveEvidenceSummary::default(), NOW))
            .await
            .unwrap();
        assert_eq!(first.classification, GateClassification::Stable);
        assert_eq!(first.baseline_latency_ms, None);

        let second = gate
            .evaluate(gate_request(
                PassiveEvidenceSummary::default(),
                first.generated_at_monotonic_ms.saturating_add(1),
            ))
            .await
            .unwrap();
        assert_eq!(second.baseline_latency_ms, Some(110));
        assert_eq!(second.slow_threshold_ms, Some(1_610));
    }

    #[tokio::test]
    async fn process_baseline_clock_survives_manager_clock_restart() {
        let mut gate = EnvironmentGate::with_backend(fake_backend());
        let first = gate
            .evaluate_with_baseline_clock(
                gate_request(PassiveEvidenceSummary::default(), 8_000),
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(first.baseline_latency_ms, None);

        // A new Eyes/Manager generation starts its report clock near zero,
        // while the retained Gate continues on the process-wide epoch.
        let second = gate
            .evaluate_with_baseline_clock(
                gate_request(PassiveEvidenceSummary::default(), 5),
                NOW + 1_000,
            )
            .await
            .unwrap();
        assert_eq!(second.baseline_latency_ms, Some(110));
        assert!(second.generated_at_monotonic_ms < 1_000);
    }

    #[tokio::test]
    async fn first_suspect_round_cannot_seed_or_classify_itself() {
        let reset = PassiveEvidenceSummary {
            reset_after_client_hello_flows: 3,
            reset_targets: 2,
            ..PassiveEvidenceSummary::default()
        };
        let mut gate = EnvironmentGate::with_backend(fake_backend());
        let first = gate.evaluate(gate_request(reset, NOW)).await.unwrap();
        assert_eq!(first.classification, GateClassification::Stable);
        assert_eq!(first.baseline_latency_ms, None);
        assert_eq!(gate.baselines_mut().sample_count(NOW), 0);
    }

    #[tokio::test]
    async fn unreliable_sensor_round_cannot_seed_baseline() {
        let mut gate = EnvironmentGate::with_backend(fake_backend());
        let mut request = gate_request(PassiveEvidenceSummary::default(), NOW);
        request.sensor.state = EyeHealthState::Degraded;
        let report = gate.evaluate(request).await.unwrap();
        assert_eq!(report.classification, GateClassification::SensorUnreliable);
        assert_eq!(gate.baselines_mut().sample_count(NOW), 0);
    }

    #[derive(Clone, Debug)]
    struct BarrierBackend {
        barrier: Arc<Barrier>,
    }

    impl EndpointProbeBackend for BarrierBackend {
        fn probe(&self, endpoint: String, _timeout: Duration) -> EndpointProbeFuture<'_> {
            let barrier = Arc::clone(&self.barrier);
            Box::pin(async move {
                barrier.wait().await;
                EndpointProbeOutcome::http(endpoint, 10, 204)
            })
        }
    }

    #[tokio::test]
    async fn controls_and_category_targets_start_concurrently() {
        let backend = BarrierBackend {
            barrier: Arc::new(Barrier::new(5)),
        };
        let mut gate = EnvironmentGate::with_backend(backend);
        let mut request = gate_request(PassiveEvidenceSummary::default(), NOW);
        request.category_targets = vec!["one.example".into(), "two.example".into()];

        let report = tokio::time::timeout(Duration::from_millis(500), gate.evaluate(request))
            .await
            .expect("all five probes must reach the barrier concurrently")
            .unwrap();
        assert_eq!(report.controls.len(), 3);
        assert_eq!(report.category_targets.len(), 2);
    }

    #[derive(Clone, Debug)]
    struct StageAwareTimeoutBackend;

    impl EndpointProbeBackend for StageAwareTimeoutBackend {
        fn probe(&self, endpoint: String, timeout: Duration) -> EndpointProbeFuture<'_> {
            Box::pin(async move {
                // Model a production request timeout that becomes runnable a
                // little after its own budget. An equal outer timeout must not
                // replace this stage-aware result with a DNS timeout.
                tokio::time::sleep(timeout + Duration::from_millis(1)).await;
                EndpointProbeOutcome::timed_out(
                    endpoint,
                    EndpointProbeStage::Transport,
                    duration_ms(timeout),
                )
            })
        }
    }

    #[tokio::test]
    async fn backend_timeout_keeps_its_transport_stage_at_endpoint_boundary() {
        let endpoint_timeout = Duration::from_millis(5);
        let (controls, categories) = probe_round_with_deadline(
            Arc::new(StageAwareTimeoutBackend),
            &[],
            endpoint_timeout,
            Duration::from_millis(50),
        )
        .await;

        assert!(categories.is_empty());
        assert_eq!(controls.len(), CONTROL_ENDPOINTS.len());
        assert!(controls.iter().all(|outcome| {
            outcome.stage == EndpointProbeStage::Transport
                && outcome.result == EndpointProbeResult::TimedOut
                && outcome.dns_succeeded()
        }));
        assert_eq!(
            classify(
                &local_network(),
                ready_sensor(),
                PassiveEvidenceSummary::default(),
                &controls,
                &reachable_targets(),
                Some(100),
            ),
            GateClassification::UpstreamDegraded
        );
    }

    #[derive(Clone, Debug)]
    struct HangingBackend;

    impl EndpointProbeBackend for HangingBackend {
        fn probe(&self, _endpoint: String, _timeout: Duration) -> EndpointProbeFuture<'_> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn global_deadline_bounds_hung_backend_without_claiming_dns_failure() {
        let global_deadline = Duration::from_millis(10);
        let (controls, categories) = tokio::time::timeout(
            Duration::from_millis(250),
            probe_round_with_deadline(
                Arc::new(HangingBackend),
                &["video.example".to_owned()],
                Duration::from_millis(5),
                global_deadline,
            ),
        )
        .await
        .expect("global deadline must fence a non-cooperative backend");

        assert_eq!(controls.len(), CONTROL_ENDPOINTS.len());
        assert_eq!(categories.len(), 1);
        assert!(controls.iter().chain(&categories).all(|outcome| {
            outcome.stage == EndpointProbeStage::Transport
                && outcome.result == EndpointProbeResult::TimedOut
                && outcome.duration_ms == duration_ms(global_deadline)
        }));
    }

    #[test]
    fn request_rejects_oversized_target_set_and_fingerprint_race() {
        let mut request = gate_request(PassiveEvidenceSummary::default(), NOW);
        request.category_targets = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];
        assert_eq!(
            request.validate(),
            Err(GateRequestError::TooManyCategoryTargets {
                actual: 3,
                maximum: 2,
            })
        );

        request.category_targets.truncate(1);
        request.local_network.network_fingerprint = NetworkFingerprint::Unknown;
        assert_eq!(
            request.validate(),
            Err(GateRequestError::NetworkFingerprintMismatch)
        );
    }

    #[test]
    fn only_https_targets_are_accepted_by_production_normalization() {
        assert_eq!(
            normalized_https_url("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert!(normalized_https_url("https://example.com/path").is_ok());
        assert!(normalized_https_url("http://example.com").is_err());
        assert!(normalized_https_url("not a host").is_err());
    }
}
