//! Transport-specific active probes для оценки adaptive Zapret2 candidate.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error as _;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;

use super::dsl::{AdaptiveCategory, StrategyTransport};
use super::evidence::{FailureStage, SeriesVerdict};
use super::model::CandidateProbeResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeTarget {
    pub host: &'static str,
    pub port: u16,
    pub path: &'static str,
    pub core: bool,
}

type DnsCache = Arc<tokio::sync::Mutex<HashMap<&'static str, Vec<SocketAddr>>>>;

#[derive(Clone, Copy, Debug)]
struct ProbeBudget {
    deadline: Instant,
}

impl ProbeBudget {
    fn new(timeout: Duration) -> Self {
        Self {
            deadline: Instant::now() + timeout,
        }
    }

    fn remaining(self) -> Option<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
    }

    async fn timeout<F>(self, future: F) -> Result<F::Output, tokio::time::error::Elapsed>
    where
        F: Future,
    {
        tokio::time::timeout_at(self.deadline, future).await
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionDnsCache {
    inner: DnsCache,
}

impl SessionDnsCache {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    async fn address_count(&self, host: &'static str) -> usize {
        self.inner.lock().await.get(host).map_or(0, Vec::len)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetProbeResult {
    pub host: String,
    pub core: bool,
    pub round: u8,
    pub transport: StrategyTransport,
    pub dns_ok: bool,
    pub tcp_ok: bool,
    pub tls_ok: bool,
    pub quic_ok: bool,
    pub https_ok: bool,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub failure_stage: FailureStage,
    pub detail: String,
}

impl TargetProbeResult {
    fn final_ok(&self) -> bool {
        match self.transport {
            StrategyTransport::Tls => self.dns_ok && self.tcp_ok && self.tls_ok && self.https_ok,
            StrategyTransport::Quic => self.dns_ok && self.quic_ok && self.https_ok,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeBatch {
    pub category: AdaptiveCategory,
    pub transport: StrategyTransport,
    pub round: u8,
    pub targets: Vec<TargetProbeResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSeries {
    pub category: AdaptiveCategory,
    pub transport: StrategyTransport,
    pub required_successes: u8,
    pub rounds: Vec<ProbeBatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EyesProbeEvidence {
    pub reset_count: u32,
    pub blackhole_count: u32,
    pub working_by_host: BTreeMap<String, u32>,
}

#[derive(Clone, Copy)]
enum CoreRequirement {
    All,
    Any,
}

impl ProbeSeries {
    pub fn evaluate(&self, eyes: &EyesProbeEvidence) -> CandidateProbeResult {
        self.evaluate_with_requirement(
            eyes,
            if self.transport == StrategyTransport::Quic {
                CoreRequirement::Any
            } else {
                CoreRequirement::All
            },
        )
    }

    pub fn evaluate_base(&self, eyes: &EyesProbeEvidence) -> CandidateProbeResult {
        self.evaluate_with_requirement(eyes, CoreRequirement::Any)
    }

    pub fn stable_core_targets(
        &self,
        configured_targets: &[ProbeTarget],
        eyes: &EyesProbeEvidence,
    ) -> Vec<ProbeTarget> {
        let required = self.required_successes.max(1) as usize;
        configured_targets
            .iter()
            .copied()
            .filter(|target| target.core)
            .filter(|target| {
                let direct_successes = self
                    .rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|result| result.host == target.host && result.final_ok())
                    .count();
                let eyes_successes = if self.transport == StrategyTransport::Tls {
                    eyes.working_by_host.get(target.host).copied().unwrap_or(0) as usize
                } else {
                    0
                };
                direct_successes.max(eyes_successes) >= required
            })
            .collect()
    }

    pub fn restricted_to(&self, targets: &[ProbeTarget]) -> Self {
        let selected = targets
            .iter()
            .map(|target| target.host)
            .collect::<BTreeSet<_>>();
        let mut restricted = self.clone();
        for batch in &mut restricted.rounds {
            batch
                .targets
                .retain(|target| selected.contains(target.host.as_str()));
        }
        restricted
    }

    fn evaluate_with_requirement(
        &self,
        eyes: &EyesProbeEvidence,
        requirement: CoreRequirement,
    ) -> CandidateProbeResult {
        let required = self.required_successes.max(1);
        let core_hosts = self
            .rounds
            .iter()
            .flat_map(|batch| batch.targets.iter())
            .filter(|target| target.core)
            .map(|target| target.host.clone())
            .collect::<BTreeSet<_>>();

        let round_passes = |predicate: fn(&TargetProbeResult) -> bool| {
            self.rounds
                .iter()
                .filter(|batch| {
                    let mut core = batch.targets.iter().filter(|target| target.core);
                    let has_core = core.clone().next().is_some();
                    has_core
                        && match requirement {
                            CoreRequirement::All => core.all(predicate),
                            CoreRequirement::Any => core.any(predicate),
                        }
                })
                .count() as u8
        };

        let stage_passes = |predicate: fn(&TargetProbeResult) -> bool| {
            let host_passes = |host: &str| {
                self.rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|target| target.core && target.host == host && predicate(target))
                    .count() as u8
                    >= required
            };
            !core_hosts.is_empty()
                && match requirement {
                    CoreRequirement::All => core_hosts.iter().all(|host| host_passes(host)),
                    CoreRequirement::Any => core_hosts.iter().any(|host| host_passes(host)),
                }
        };

        let eyes_working_hosts = core_hosts
            .iter()
            .filter(|host| eyes.working_by_host.get(*host).copied().unwrap_or(0) >= required as u32)
            .count()
            .min(u8::MAX as usize) as u8;
        let eyes_working_count = match requirement {
            CoreRequirement::All => core_hosts
                .iter()
                .map(|host| eyes.working_by_host.get(host).copied().unwrap_or(0))
                .min()
                .unwrap_or(0),
            CoreRequirement::Any => core_hosts
                .iter()
                .map(|host| eyes.working_by_host.get(host).copied().unwrap_or(0))
                .max()
                .unwrap_or(0),
        };
        let eyes_working_ok = self.transport == StrategyTransport::Tls
            && !core_hosts.is_empty()
            && match requirement {
                CoreRequirement::All => eyes_working_hosts as usize == core_hosts.len(),
                CoreRequirement::Any => eyes_working_hosts > 0,
            };

        let dns_ok = stage_passes(|target| target.dns_ok);
        let tcp_ok =
            self.transport == StrategyTransport::Quic || stage_passes(|target| target.tcp_ok);
        let tls_or_quic_ok = match self.transport {
            StrategyTransport::Tls => stage_passes(|target| target.tls_ok) || eyes_working_ok,
            StrategyTransport::Quic => stage_passes(|target| target.quic_ok),
        };
        let https_ok = stage_passes(|target| target.https_ok);
        let successful_rounds = round_passes(TargetProbeResult::final_ok).max(
            eyes_working_count
                .min(self.rounds.len() as u32)
                .min(u8::MAX as u32) as u8,
        );
        let application_ok = https_ok || eyes_working_ok;

        let failure_stage = if eyes.blackhole_count >= 2 {
            FailureStage::EyesBlackhole
        } else if eyes.reset_count >= 2 {
            FailureStage::EyesReset
        } else if !dns_ok {
            FailureStage::Dns
        } else if !tcp_ok {
            FailureStage::Tcp
        } else if !tls_or_quic_ok {
            match self.transport {
                StrategyTransport::Tls => FailureStage::Tls,
                StrategyTransport::Quic => FailureStage::Quic,
            }
        } else if !application_ok || successful_rounds < required {
            FailureStage::Https
        } else {
            FailureStage::None
        };

        CandidateProbeResult {
            transport: self.transport,
            dns_ok,
            tcp_ok,
            tls_or_quic_ok,
            https_ok,
            eyes_working_ok,
            eyes_working_count,
            eyes_working_hosts,
            reset_count: eyes.reset_count,
            blackhole_count: eyes.blackhole_count,
            successful_rounds,
            required_successes: required,
            total_rounds: self.rounds.len().min(u8::MAX as usize) as u8,
            failure_stage,
        }
    }
}

pub fn targets_for(category: AdaptiveCategory, transport: StrategyTransport) -> Vec<ProbeTarget> {
    match category {
        AdaptiveCategory::YoutubeTwitch => vec![
            ProbeTarget {
                host: "www.youtube.com",
                port: 443,
                path: "/",
                core: true,
            },
            ProbeTarget {
                host: "youtube.com",
                port: 443,
                path: "/",
                core: true,
            },
            ProbeTarget {
                host: "i.ytimg.com",
                port: 443,
                path: "/",
                core: false,
            },
        ],
        AdaptiveCategory::Discord => vec![
            ProbeTarget {
                host: "discord.com",
                port: 443,
                path: "/",
                core: true,
            },
            ProbeTarget {
                host: "gateway.discord.gg",
                port: 443,
                path: "/",
                core: true,
            },
        ],
        AdaptiveCategory::Gaming => match transport {
            StrategyTransport::Tls => vec![
                ProbeTarget {
                    host: "github.com",
                    port: 443,
                    path: "/",
                    core: true,
                },
                ProbeTarget {
                    host: "www.roblox.com",
                    port: 443,
                    path: "/",
                    core: true,
                },
                ProbeTarget {
                    host: "api.github.com",
                    port: 443,
                    path: "/",
                    core: false,
                },
                ProbeTarget {
                    host: "www.epicgames.com",
                    port: 443,
                    path: "/",
                    core: false,
                },
            ],
            StrategyTransport::Quic => vec![
                ProbeTarget {
                    host: "www.roblox.com",
                    port: 443,
                    path: "/",
                    core: true,
                },
                ProbeTarget {
                    host: "www.epicgames.com",
                    port: 443,
                    path: "/",
                    core: true,
                },
                ProbeTarget {
                    host: "github.com",
                    port: 443,
                    path: "/",
                    core: false,
                },
                ProbeTarget {
                    host: "api.github.com",
                    port: 443,
                    path: "/",
                    core: false,
                },
            ],
        },
    }
}

pub fn alt_svc_supports_h3(value: &str) -> bool {
    value.split(',').any(|alternative| {
        alternative
            .split_once('=')
            .map(|(protocol, _)| protocol.trim().trim_matches('"').to_ascii_lowercase())
            .is_some_and(|protocol| protocol == "h3" || protocol.starts_with("h3-"))
    })
}

#[allow(dead_code)]
pub async fn discover_quic_targets(
    category: AdaptiveCategory,
    timeout: Duration,
) -> Vec<ProbeTarget> {
    discover_quic_targets_with_cache(category, timeout, &SessionDnsCache::new()).await
}

pub async fn discover_quic_targets_with_cache(
    category: AdaptiveCategory,
    timeout: Duration,
    dns_cache: &SessionDnsCache,
) -> Vec<ProbeTarget> {
    let mut set = tokio::task::JoinSet::new();
    for target in targets_for(category, StrategyTransport::Tls) {
        let dns_cache = dns_cache.clone();
        set.spawn(async move {
            let budget = ProbeBudget::new(timeout);
            let addresses = resolve_cached_addresses(&target, budget, dns_cache.inner.clone())
                .await
                .ok()?;
            let remaining = budget.remaining()?;
            let authority = if target.port == 443 {
                target.host.to_string()
            } else {
                format!("{}:{}", target.host, target.port)
            };
            let url = format!("https://{authority}{}", target.path);
            let client = reqwest::Client::builder()
                .connect_timeout(remaining)
                .timeout(remaining)
                .redirect(reqwest::redirect::Policy::none())
                .resolve_to_addrs(target.host, &addresses)
                .build()
                .ok()?;
            let response = budget.timeout(client.get(url).send()).await.ok()?.ok()?;
            if !(200..=499).contains(&response.status().as_u16()) {
                return None;
            }
            response
                .headers()
                .get_all("alt-svc")
                .iter()
                .filter_map(|value| value.to_str().ok())
                .any(alt_svc_supports_h3)
                .then_some(ProbeTarget {
                    core: true,
                    ..target
                })
        });
    }

    let mut targets = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(Some(target)) = result {
            targets.push(target);
        }
    }
    targets.sort_by(|left, right| left.host.cmp(right.host));
    targets.truncate(2);
    targets
}

#[allow(dead_code)]
pub async fn run_probe_series(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    timeout: Duration,
    round_count: u8,
    required_successes: u8,
    interval: Duration,
) -> ProbeSeries {
    let targets = targets_for(category, transport);
    run_probe_series_for_targets(
        category,
        transport,
        &targets,
        timeout,
        round_count,
        required_successes,
        interval,
    )
    .await
}

pub async fn run_probe_series_for_targets(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    targets: &[ProbeTarget],
    timeout: Duration,
    round_count: u8,
    required_successes: u8,
    interval: Duration,
) -> ProbeSeries {
    run_probe_series_for_targets_with_cache(
        category,
        transport,
        targets,
        timeout,
        round_count,
        required_successes,
        interval,
        &SessionDnsCache::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_probe_series_for_targets_with_cache(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    targets: &[ProbeTarget],
    timeout: Duration,
    round_count: u8,
    required_successes: u8,
    interval: Duration,
    dns_cache: &SessionDnsCache,
) -> ProbeSeries {
    run_probe_series_for_targets_with_progress_and_cache(
        category,
        transport,
        targets,
        timeout,
        round_count,
        required_successes,
        interval,
        dns_cache,
        |_| {},
    )
    .await
}

#[allow(dead_code, clippy::too_many_arguments)]
pub async fn run_probe_series_for_targets_with_progress<F>(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    targets: &[ProbeTarget],
    timeout: Duration,
    round_count: u8,
    required_successes: u8,
    interval: Duration,
    on_round: F,
) -> ProbeSeries
where
    F: FnMut(&ProbeBatch),
{
    run_probe_series_for_targets_with_progress_and_cache(
        category,
        transport,
        targets,
        timeout,
        round_count,
        required_successes,
        interval,
        &SessionDnsCache::new(),
        on_round,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_probe_series_for_targets_with_progress_and_cache<F>(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    targets: &[ProbeTarget],
    timeout: Duration,
    round_count: u8,
    required_successes: u8,
    interval: Duration,
    dns_cache: &SessionDnsCache,
    mut on_round: F,
) -> ProbeSeries
where
    F: FnMut(&ProbeBatch),
{
    let mut rounds = Vec::with_capacity(round_count as usize);
    for round in 1..=round_count {
        rounds.push(
            run_probe_round(
                category,
                transport,
                targets,
                round,
                timeout,
                dns_cache.inner.clone(),
            )
            .await,
        );
        on_round(rounds.last().expect("probe round was just appended"));
        let remaining = round_count.saturating_sub(round);
        if series_verdict_state(&rounds, transport, required_successes, remaining)
            != SeriesVerdict::Undecided
        {
            break;
        }
        if round < round_count {
            tokio::time::sleep(interval).await;
        }
    }
    ProbeSeries {
        category,
        transport,
        required_successes,
        rounds,
    }
}
async fn run_probe_round(
    category: AdaptiveCategory,
    transport: StrategyTransport,
    probe_targets: &[ProbeTarget],
    round: u8,
    timeout: Duration,
    dns_cache: DnsCache,
) -> ProbeBatch {
    let mut set = tokio::task::JoinSet::new();
    for target in probe_targets.iter().copied() {
        let dns_cache = dns_cache.clone();
        set.spawn(async move {
            probe_target_cached(target, transport, round, timeout, dns_cache).await
        });
    }
    let mut targets = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(result) = result {
            targets.push(result);
        }
    }
    targets.sort_by(|left, right| left.host.cmp(&right.host));
    ProbeBatch {
        category,
        transport,
        round,
        targets,
    }
}

fn series_verdict_state(
    rounds: &[ProbeBatch],
    transport: StrategyTransport,
    required_successes: u8,
    remaining_rounds: u8,
) -> SeriesVerdict {
    let required = required_successes.max(1);
    let has_core_targets = rounds
        .iter()
        .flat_map(|batch| batch.targets.iter())
        .any(|target| target.core);
    if !has_core_targets {
        return SeriesVerdict::Undecided;
    }
    let requirement = if transport == StrategyTransport::Quic {
        CoreRequirement::Any
    } else {
        CoreRequirement::All
    };
    let successful_rounds = rounds
        .iter()
        .filter(|batch| {
            let mut core = batch.targets.iter().filter(|target| target.core);
            let has_core = core.clone().next().is_some();
            has_core
                && match requirement {
                    CoreRequirement::All => core.all(TargetProbeResult::final_ok),
                    CoreRequirement::Any => core.any(TargetProbeResult::final_ok),
                }
        })
        .count()
        .min(u8::MAX as usize) as u8;

    if successful_rounds >= required {
        SeriesVerdict::FinalSuccess
    } else if transport == StrategyTransport::Quic
        && successful_rounds.saturating_add(remaining_rounds) < required
    {
        SeriesVerdict::FinalFailure
    } else {
        SeriesVerdict::Undecided
    }
}

#[cfg(test)]
async fn probe_target(
    target: ProbeTarget,
    transport: StrategyTransport,
    round: u8,
    timeout: Duration,
) -> TargetProbeResult {
    probe_target_cached(
        target,
        transport,
        round,
        timeout,
        Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    )
    .await
}

async fn resolve_cached_addresses(
    target: &ProbeTarget,
    budget: ProbeBudget,
    dns_cache: DnsCache,
) -> Result<Vec<SocketAddr>, String> {
    let cached = budget
        .timeout(dns_cache.lock())
        .await
        .map_err(|_| "dns: target timeout".to_string())?
        .get(target.host)
        .cloned();
    if let Some(addresses) = cached {
        return Ok(addresses);
    }
    let addresses = resolve_addresses(target, budget).await?;
    if let Ok(mut cache) = budget.timeout(dns_cache.lock()).await {
        cache.insert(target.host, addresses.clone());
    }
    Ok(addresses)
}

async fn probe_target_cached(
    target: ProbeTarget,
    transport: StrategyTransport,
    round: u8,
    timeout: Duration,
    dns_cache: DnsCache,
) -> TargetProbeResult {
    let started = Instant::now();
    let budget = ProbeBudget::new(timeout);
    let mut result = TargetProbeResult {
        host: target.host.to_string(),
        core: target.core,
        round,
        transport,
        dns_ok: false,
        tcp_ok: false,
        tls_ok: false,
        quic_ok: false,
        https_ok: false,
        http_status: None,
        latency_ms: 0,
        failure_stage: FailureStage::Dns,
        detail: String::new(),
    };

    let addresses = match resolve_cached_addresses(&target, budget, dns_cache).await {
        Ok(addresses) => addresses,
        Err(error) => {
            result.detail = error;
            return finish(result, started);
        }
    };
    result.dns_ok = true;

    if transport == StrategyTransport::Quic {
        let Some(remaining) = budget.remaining() else {
            result.failure_stage = FailureStage::Quic;
            result.detail = "quic: target timeout exhausted during DNS".into();
            return finish(result, started);
        };
        match probe_quic(&addresses, target.host, remaining).await {
            Ok(()) => {
                result.quic_ok = true;
                result.https_ok = true;
                result.failure_stage = FailureStage::None;
                result.detail = "QUIC Initial acknowledged".into();
            }
            Err(error) => {
                result.failure_stage = FailureStage::Quic;
                result.detail = format!("quic: {error}");
            }
        }
        return finish(result, started);
    }

    let Some(remaining) = budget.remaining() else {
        result.failure_stage = FailureStage::Tcp;
        result.detail = "tcp: target timeout exhausted during DNS".into();
        return finish(result, started);
    };
    result.failure_stage = FailureStage::Tcp;
    let builder = reqwest::Client::builder()
        .connect_timeout(remaining)
        .timeout(remaining)
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(target.host, &addresses);
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            result.failure_stage = match transport {
                StrategyTransport::Tls => FailureStage::Tls,
                StrategyTransport::Quic => FailureStage::Quic,
            };
            result.detail = format!("client: {error}");
            return finish(result, started);
        }
    };
    let authority = if target.port == 443 {
        target.host.to_string()
    } else {
        format!("{}:{}", target.host, target.port)
    };
    let url = format!("https://{authority}{}", target.path);
    match budget.timeout(client.get(url).send()).await {
        Ok(Ok(response)) => {
            match transport {
                StrategyTransport::Tls => {
                    result.tcp_ok = true;
                    result.tls_ok = true;
                }
                StrategyTransport::Quic => result.quic_ok = true,
            }
            let status = response.status().as_u16();
            result.http_status = Some(status);
            result.https_ok = (200..=499).contains(&status);
            result.failure_stage = if result.https_ok {
                FailureStage::None
            } else {
                FailureStage::Https
            };
            result.detail = format!(
                "{} {}",
                if transport == StrategyTransport::Quic {
                    "HTTP/3"
                } else {
                    "HTTPS"
                },
                status
            );
        }
        Ok(Err(error)) => {
            result.failure_stage = classify_reqwest_error(&error);
            result.tcp_ok = transport == StrategyTransport::Tls
                && (result.failure_stage == FailureStage::Tls
                    || reqwest_error_establishes_tcp(&error));
            result.detail = format!(
                "{}: {}",
                result.failure_stage.as_str(),
                reqwest_error_chain(&error)
            );
        }
        Err(_) => {
            result.failure_stage = FailureStage::Tcp;
            result.detail = "tcp: target timeout".into();
        }
    }
    finish(result, started)
}

fn bounded_addresses(addresses: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let addresses = addresses.into_iter().collect::<Vec<_>>();
    addresses
        .iter()
        .find(|address| address.is_ipv6())
        .into_iter()
        .chain(addresses.iter().find(|address| address.is_ipv4()))
        .copied()
        .collect()
}

async fn resolve_addresses(
    target: &ProbeTarget,
    budget: ProbeBudget,
) -> Result<Vec<SocketAddr>, String> {
    let mut last_error = "dns: no addresses".to_string();
    for attempt in 0..2 {
        match budget
            .timeout(tokio::net::lookup_host((target.host, target.port)))
            .await
        {
            Ok(Ok(addresses)) => {
                let addresses = bounded_addresses(addresses);
                if !addresses.is_empty() {
                    return Ok(addresses);
                }
                last_error = "dns: no addresses".into();
            }
            Ok(Err(error)) => last_error = format!("dns: {error}"),
            Err(_) => last_error = "dns: timeout".into(),
        }
        if attempt == 0
            && budget
                .timeout(tokio::time::sleep(Duration::from_millis(150)))
                .await
                .is_err()
        {
            return Err("dns: target timeout".into());
        }
    }
    Err(last_error)
}
fn classify_reqwest_error(error: &reqwest::Error) -> FailureStage {
    if error.is_body() || error.is_decode() || error.status().is_some() {
        return FailureStage::Https;
    }
    if error.is_connect() {
        let chain = reqwest_error_chain(error).to_ascii_lowercase();
        if chain.contains("tls")
            || chain.contains("certificate")
            || chain.contains("handshake")
            || chain.contains("rustls")
        {
            return FailureStage::Tls;
        }
        return FailureStage::Tcp;
    }
    FailureStage::Https
}

fn reqwest_error_establishes_tcp(error: &reqwest::Error) -> bool {
    let mut source = error.source();
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            ) {
                return true;
            }
        }
        source = error.source();
    }
    // hyper иногда стирает concrete io::Error при boxing, но std::io::Error
    // сохраняет стабильный WSA-код в display даже при локализованном сообщении.
    // 10054/10053 = reset/abort уже установленного TCP; 10061 (refused) сюда
    // намеренно не входит.
    let chain = reqwest_error_chain(error).to_ascii_lowercase();
    chain.contains("os error 10054") || chain.contains("os error 10053")
}

fn reqwest_error_chain(error: &reqwest::Error) -> String {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        let message = error.to_string();
        if chain.last() != Some(&message) {
            chain.push(message);
        }
        source = error.source();
    }
    chain.join(" | caused by: ")
}
async fn probe_quic(
    addresses: &[std::net::SocketAddr],
    server_name: &str,
    timeout: Duration,
) -> Result<(), String> {
    use std::sync::Arc;

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"h3".to_vec()];
    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|error| format!("TLS config: {error}"))?;
    let client_config = quinn::ClientConfig::new(Arc::new(quic_crypto));

    let selected = addresses
        .iter()
        .find(|address| address.is_ipv6())
        .into_iter()
        .chain(addresses.iter().find(|address| address.is_ipv4()))
        .copied()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("no addresses".into());
    }

    let server_name = server_name.to_string();
    let attempt = async move {
        let mut set = tokio::task::JoinSet::new();
        for (index, address) in selected.into_iter().enumerate() {
            let client_config = client_config.clone();
            let server_name = server_name.clone();
            set.spawn(async move {
                if index > 0 {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                let bind = if address.is_ipv4() {
                    "0.0.0.0:0"
                } else {
                    "[::]:0"
                }
                .parse()
                .map_err(|error| format!("bind address: {error}"))?;
                let mut endpoint =
                    quinn::Endpoint::client(bind).map_err(|error| format!("endpoint: {error}"))?;
                endpoint.set_default_client_config(client_config);
                let connecting = endpoint
                    .connect(address, &server_name)
                    .map_err(|error| format!("connect setup: {error}"))?;
                let connection = connecting.await.map_err(|error| error.to_string())?;
                connection.close(0u32.into(), b"adaptive probe complete");
                Ok::<(), String>(())
            });
        }

        let mut last_error = "all address attempts failed".to_string();
        while let Some(result) = set.join_next().await {
            match result {
                Ok(Ok(())) => {
                    set.abort_all();
                    return Ok(());
                }
                Ok(Err(error)) => last_error = error,
                Err(error) => last_error = error.to_string(),
            }
        }
        Err(last_error)
    };

    tokio::time::timeout(timeout, attempt)
        .await
        .map_err(|_| "timeout".to_string())?
}
fn finish(mut result: TargetProbeResult, started: Instant) -> TargetProbeResult {
    result.latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(round: u8, host: &str, core: bool, ok: bool) -> TargetProbeResult {
        TargetProbeResult {
            host: host.into(),
            core,
            round,
            transport: StrategyTransport::Tls,
            dns_ok: ok,
            tcp_ok: ok,
            tls_ok: ok,
            quic_ok: false,
            https_ok: ok,
            http_status: ok.then_some(200),
            latency_ms: 1,
            failure_stage: if ok {
                FailureStage::None
            } else {
                FailureStage::Https
            },
            detail: String::new(),
        }
    }

    fn batch(round: u8, first_ok: bool, second_ok: bool) -> ProbeBatch {
        ProbeBatch {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Tls,
            round,
            targets: vec![
                result(round, "youtube.com", true, first_ok),
                result(round, "www.youtube.com", true, second_ok),
                result(round, "i.ytimg.com", false, false),
            ],
        }
    }

    #[test]
    fn target_sets_are_small_stable_and_category_specific() {
        let youtube = targets_for(AdaptiveCategory::YoutubeTwitch, StrategyTransport::Tls);
        assert_eq!(youtube[0].host, "www.youtube.com");
        assert!(youtube.iter().any(|target| !target.core));
        let discord = targets_for(AdaptiveCategory::Discord, StrategyTransport::Tls);
        assert_eq!(discord.len(), 2);
        assert!(discord.iter().all(|target| target.core));

        let gaming_tls = targets_for(AdaptiveCategory::Gaming, StrategyTransport::Tls);
        assert_eq!(gaming_tls.len(), 4);
        assert!(gaming_tls
            .iter()
            .any(|target| target.host == "github.com" && target.core));
        assert!(gaming_tls
            .iter()
            .any(|target| target.host == "www.roblox.com" && target.core));

        let gaming_quic = targets_for(AdaptiveCategory::Gaming, StrategyTransport::Quic);
        assert_eq!(gaming_quic.len(), 4);
        assert!(gaming_quic
            .iter()
            .any(|target| target.host == "github.com" && !target.core));
        assert!(gaming_quic
            .iter()
            .any(|target| target.host == "www.roblox.com" && target.core));
        assert!(gaming_quic
            .iter()
            .any(|target| target.host == "www.epicgames.com" && target.core));
    }

    #[test]
    fn evaluator_tolerates_one_transient_round_and_optional_failure() {
        let series = ProbeSeries {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds: vec![
                batch(1, true, true),
                batch(2, false, true),
                batch(3, true, true),
            ],
        };
        let result = series.evaluate(&EyesProbeEvidence::default());
        assert!(result.is_success());
        assert_eq!(result.successful_rounds, 2);
    }

    #[test]
    fn repeated_eyes_evidence_vetoes_http_success() {
        let series = ProbeSeries {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds: vec![
                batch(1, true, true),
                batch(2, true, true),
                batch(3, true, true),
            ],
        };
        let result = series.evaluate(&EyesProbeEvidence {
            reset_count: 2,
            ..Default::default()
        });
        assert!(!result.is_success());
        assert_eq!(result.failure_stage, FailureStage::EyesReset);
    }

    #[test]
    fn quic_requires_quic_evidence_not_tls_flags() {
        let mut target = result(1, "youtube.com", true, true);
        target.transport = StrategyTransport::Quic;
        target.tcp_ok = false;
        target.quic_ok = false;
        let series = ProbeSeries {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Quic,
            required_successes: 1,
            rounds: vec![ProbeBatch {
                category: AdaptiveCategory::YoutubeTwitch,
                transport: StrategyTransport::Quic,
                round: 1,
                targets: vec![target],
            }],
        };
        let result = series.evaluate(&EyesProbeEvidence::default());
        assert!(!result.is_success());
        assert_eq!(result.failure_stage, FailureStage::Quic);
    }

    #[test]
    fn quic_candidate_accepts_any_confirmed_core_target() {
        let target = |round: u8, host: &str, ok: bool| {
            let mut target = result(round, host, true, ok);
            target.transport = StrategyTransport::Quic;
            target.tls_ok = false;
            target.quic_ok = ok;
            target.https_ok = ok;
            target.failure_stage = if ok {
                FailureStage::None
            } else {
                FailureStage::Quic
            };
            target
        };
        let series = ProbeSeries {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Quic,
            required_successes: 2,
            rounds: vec![
                ProbeBatch {
                    category: AdaptiveCategory::YoutubeTwitch,
                    transport: StrategyTransport::Quic,
                    round: 1,
                    targets: vec![
                        target(1, "i.ytimg.com", false),
                        target(1, "www.youtube.com", true),
                    ],
                },
                ProbeBatch {
                    category: AdaptiveCategory::YoutubeTwitch,
                    transport: StrategyTransport::Quic,
                    round: 2,
                    targets: vec![
                        target(2, "i.ytimg.com", false),
                        target(2, "www.youtube.com", true),
                    ],
                },
            ],
        };
        let evaluated = series.evaluate(&EyesProbeEvidence::default());
        assert!(evaluated.is_success());
        assert_eq!(evaluated.successful_rounds, 2);
    }

    fn discord_failed_round(round: u8, dns_ok: bool) -> ProbeBatch {
        let target = |host: &str| TargetProbeResult {
            host: host.into(),
            core: true,
            round,
            transport: StrategyTransport::Tls,
            dns_ok,
            tcp_ok: dns_ok,
            tls_ok: false,
            quic_ok: false,
            https_ok: false,
            http_status: None,
            latency_ms: if dns_ok { 100 } else { 5_000 },
            failure_stage: if dns_ok {
                FailureStage::Https
            } else {
                FailureStage::Dns
            },
            detail: "live regression fixture".into(),
        };
        ProbeBatch {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            round,
            targets: vec![target("discord.com"), target("gateway.discord.gg")],
        }
    }

    #[test]
    fn discord_server_hello_evidence_overrides_reqwest_send_errors() {
        let series = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds: vec![
                discord_failed_round(1, false),
                discord_failed_round(2, true),
                discord_failed_round(3, true),
            ],
        };
        let eyes = EyesProbeEvidence {
            working_by_host: BTreeMap::from([
                ("discord.com".into(), 2),
                ("gateway.discord.gg".into(), 2),
            ]),
            ..Default::default()
        };

        let result = series.evaluate(&eyes);

        assert!(result.is_success());
        assert!(result.tls_or_quic_ok);
        assert!(result.eyes_working_ok);
        assert!(!result.https_ok);
        assert_eq!(result.successful_rounds, 2);
        assert_eq!(result.failure_stage, FailureStage::None);
    }

    #[test]
    fn base_recheck_accepts_one_working_discord_core_endpoint() {
        let mut gateway_ok = discord_failed_round(2, true);
        let gateway = gateway_ok
            .targets
            .iter_mut()
            .find(|target| target.host == "gateway.discord.gg")
            .unwrap();
        gateway.tls_ok = true;
        gateway.https_ok = true;
        gateway.http_status = Some(404);
        gateway.failure_stage = FailureStage::None;

        let series = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 1,
            rounds: vec![discord_failed_round(1, false), gateway_ok],
        };

        let result = series.evaluate_base(&EyesProbeEvidence::default());

        assert!(result.is_success());
        assert_eq!(result.successful_rounds, 1);
        assert_eq!(result.failure_stage, FailureStage::None);
    }

    #[test]
    fn discord_calibration_keeps_only_stable_core_targets() {
        let mut rounds = Vec::new();
        for round in 1..=4 {
            let mut batch = discord_failed_round(round, true);
            let gateway = batch
                .targets
                .iter_mut()
                .find(|target| target.host == "gateway.discord.gg")
                .unwrap();
            gateway.tls_ok = true;
            gateway.https_ok = true;
            gateway.http_status = Some(404);
            gateway.failure_stage = FailureStage::None;
            rounds.push(batch);
        }
        let series = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 3,
            rounds,
        };

        let stable = series.stable_core_targets(
            &targets_for(AdaptiveCategory::Discord, StrategyTransport::Tls),
            &EyesProbeEvidence::default(),
        );
        let restricted = series.restricted_to(&stable);

        assert_eq!(stable.len(), 1);
        assert_eq!(stable[0].host, "gateway.discord.gg");
        assert!(restricted
            .evaluate(&EyesProbeEvidence::default())
            .is_success());
    }

    #[tokio::test]
    async fn plain_tcp_listener_passes_tcp_but_fails_tls_https() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                let _socket = socket;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        let result = probe_target(
            ProbeTarget {
                host: "127.0.0.1",
                port,
                path: "/",
                core: true,
            },
            StrategyTransport::Tls,
            1,
            Duration::from_secs(1),
        )
        .await;
        assert!(result.dns_ok);
        assert!(result.tcp_ok, "{result:?}");
        assert!(!result.tls_ok);
        assert!(!result.https_ok);
        let _ = peer.await;
    }

    #[test]
    fn alt_svc_parser_accepts_http3_protocols_only() {
        assert!(alt_svc_supports_h3("h3=\":443\"; ma=86400"));
        assert!(alt_svc_supports_h3(
            "h2=\":443\"; ma=60, h3-29=\":443\"; ma=60"
        ));
        assert!(!alt_svc_supports_h3("h2=\":443\"; ma=86400"));
        assert!(!alt_svc_supports_h3("clear"));
    }

    #[tokio::test]
    async fn target_budget_caps_sequential_operations() {
        let timeout = Duration::from_millis(80);
        let started = Instant::now();
        let budget = ProbeBudget::new(timeout);

        budget
            .timeout(tokio::time::sleep(Duration::from_millis(50)))
            .await
            .unwrap();
        assert!(budget
            .timeout(tokio::time::sleep(Duration::from_millis(50)))
            .await
            .is_err());
        assert!(started.elapsed() < Duration::from_millis(140));
        assert!(budget.remaining().is_none());
    }

    #[test]
    fn dns_cache_keeps_at_most_one_address_per_family() {
        let addresses = bounded_addresses([
            "[::1]:443".parse().unwrap(),
            "[::2]:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap(),
            "127.0.0.2:443".parse().unwrap(),
        ]);
        assert_eq!(addresses.len(), 2);
        assert_eq!(
            addresses.iter().filter(|address| address.is_ipv6()).count(),
            1
        );
        assert_eq!(
            addresses.iter().filter(|address| address.is_ipv4()).count(),
            1
        );
    }

    #[tokio::test]
    async fn session_dns_cache_is_reused_across_probe_phases() {
        let cache = SessionDnsCache::new();
        let target = ProbeTarget {
            host: "127.0.0.1",
            port: 443,
            path: "/",
            core: true,
        };
        let first = resolve_cached_addresses(
            &target,
            ProbeBudget::new(Duration::from_secs(1)),
            cache.inner.clone(),
        )
        .await
        .unwrap();
        let second = resolve_cached_addresses(
            &target,
            ProbeBudget::new(Duration::from_millis(1)),
            cache.inner.clone(),
        )
        .await
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(cache.address_count(target.host).await, first.len());
        assert!(first.len() <= 2);
    }

    fn verdict_batch(transport: StrategyTransport, round: u8, core_results: &[bool]) -> ProbeBatch {
        let targets = core_results
            .iter()
            .enumerate()
            .map(|(index, ok)| {
                let mut target = result(round, &format!("core-{index}.example"), true, *ok);
                target.transport = transport;
                if transport == StrategyTransport::Quic {
                    target.tcp_ok = false;
                    target.tls_ok = false;
                    target.quic_ok = *ok;
                    target.failure_stage = if *ok {
                        FailureStage::None
                    } else {
                        FailureStage::Quic
                    };
                }
                target
            })
            .collect();
        ProbeBatch {
            category: AdaptiveCategory::Gaming,
            transport,
            round,
            targets,
        }
    }

    #[test]
    fn series_verdict_is_quorum_and_transport_aware() {
        struct Case {
            name: &'static str,
            transport: StrategyTransport,
            rounds: Vec<ProbeBatch>,
            required: u8,
            remaining: u8,
            expected: SeriesVerdict,
        }

        let cases = vec![
            Case {
                name: "tls all core final success",
                transport: StrategyTransport::Tls,
                rounds: vec![
                    verdict_batch(StrategyTransport::Tls, 1, &[true, true]),
                    verdict_batch(StrategyTransport::Tls, 2, &[true, true]),
                ],
                required: 2,
                remaining: 1,
                expected: SeriesVerdict::FinalSuccess,
            },
            Case {
                name: "tls partial core stays undecided",
                transport: StrategyTransport::Tls,
                rounds: vec![
                    verdict_batch(StrategyTransport::Tls, 1, &[true, false]),
                    verdict_batch(StrategyTransport::Tls, 2, &[true, false]),
                ],
                required: 2,
                remaining: 0,
                expected: SeriesVerdict::Undecided,
            },
            Case {
                name: "quic any core final success",
                transport: StrategyTransport::Quic,
                rounds: vec![
                    verdict_batch(StrategyTransport::Quic, 1, &[false, true]),
                    verdict_batch(StrategyTransport::Quic, 2, &[false, true]),
                ],
                required: 2,
                remaining: 1,
                expected: SeriesVerdict::FinalSuccess,
            },
            Case {
                name: "quic impossible success",
                transport: StrategyTransport::Quic,
                rounds: vec![
                    verdict_batch(StrategyTransport::Quic, 1, &[false, false]),
                    verdict_batch(StrategyTransport::Quic, 2, &[false, false]),
                ],
                required: 2,
                remaining: 1,
                expected: SeriesVerdict::FinalFailure,
            },
            Case {
                name: "quic recoverable stays undecided",
                transport: StrategyTransport::Quic,
                rounds: vec![verdict_batch(StrategyTransport::Quic, 1, &[false, false])],
                required: 1,
                remaining: 1,
                expected: SeriesVerdict::Undecided,
            },
        ];

        for case in cases {
            assert_eq!(
                series_verdict_state(&case.rounds, case.transport, case.required, case.remaining,),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
