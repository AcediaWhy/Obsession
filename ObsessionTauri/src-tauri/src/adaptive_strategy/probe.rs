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

    /// A failed TLS baseline may still be a trustworthy recovery starting
    /// point.  In particular, DPI blocking often leaves DNS healthy and lets
    /// at least one core target establish TCP before the peer resets the TLS
    /// handshake.  Treating that shape as an "unreliable environment" makes
    /// Adaptive refuse to search precisely when it is needed.
    ///
    /// Keep this deliberately stricter than the normal candidate evaluator.
    /// Either every core host must repeatedly fail after DNS while one proves
    /// an established TCP path, or one core host must remain stably healthy
    /// while another stably fails. A DNS outage, a single transient failure,
    /// or pure connect timeouts without independent control-path evidence
    /// therefore remain fail-closed.
    pub fn supports_tls_recovery(&self) -> bool {
        self.supports_tls_recovery_with_control(false)
    }

    /// Accept stable, target-specific timeout evidence only after an unrelated
    /// HTTPS control path has succeeded. This distinguishes the common DPI
    /// blackhole shape from a machine-wide offline/upstream outage while still
    /// preserving the stronger established-TCP path above.
    pub fn supports_tls_recovery_with_control(&self, control_path_ok: bool) -> bool {
        if self.transport != StrategyTransport::Tls {
            return false;
        }
        let required = self.required_successes.max(1) as usize;
        let core_hosts = self
            .rounds
            .iter()
            .flat_map(|batch| batch.targets.iter())
            .filter(|target| target.core)
            .map(|target| target.host.as_str())
            .collect::<BTreeSet<_>>();
        if core_hosts.is_empty() {
            return false;
        }

        let stable_core_successes = core_hosts
            .iter()
            .filter(|host| {
                self.rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|target| target.core && target.host == ***host && target.final_ok())
                    .count()
                    >= required
            })
            .copied()
            .collect::<BTreeSet<_>>();
        let stable_core_failures = core_hosts
            .iter()
            .filter(|host| {
                let dns_successes = self
                    .rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|target| target.core && target.host == ***host && target.dns_ok)
                    .count();
                let failed_after_dns = self
                    .rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|target| target.core && target.host == ***host)
                    .filter(|target| target.dns_ok && !target.final_ok())
                    .count();
                dns_successes >= required && failed_after_dns >= required
            })
            .copied()
            .collect::<BTreeSet<_>>();
        let stable_partial_outage = !stable_core_successes.is_empty()
            && stable_core_failures
                .iter()
                .any(|host| !stable_core_successes.contains(host));

        let every_core_is_stably_failing = core_hosts.iter().all(|host| {
            let dns_successes = self
                .rounds
                .iter()
                .flat_map(|batch| batch.targets.iter())
                .filter(|target| target.core && target.host == **host && target.dns_ok)
                .count();
            let failed_after_dns = self
                .rounds
                .iter()
                .flat_map(|batch| batch.targets.iter())
                .filter(|target| target.core && target.host == **host)
                .filter(|target| target.dns_ok && !target.final_ok())
                .count();
            dns_successes >= required && failed_after_dns >= required
        });
        let established_tcp_evidence = core_hosts.iter().any(|host| {
            self.rounds
                .iter()
                .flat_map(|batch| batch.targets.iter())
                .filter(|target| target.core && target.host == **host)
                .filter(|target| target.dns_ok && target.tcp_ok && !target.final_ok())
                .count()
                >= required
        });
        let stable_timeout_evidence = control_path_ok
            && core_hosts.iter().all(|host| {
                self.rounds
                    .iter()
                    .flat_map(|batch| batch.targets.iter())
                    .filter(|target| {
                        target.core
                            && target.host == **host
                            && target.dns_ok
                            && !target.final_ok()
                            && matches!(
                                target.failure_stage,
                                FailureStage::Tcp | FailureStage::Tls | FailureStage::Https
                            )
                    })
                    .filter(|target| {
                        let detail = target.detail.to_ascii_lowercase();
                        detail.contains("timeout") || detail.contains("timed out")
                    })
                    .count()
                    >= required
            });

        stable_partial_outage
            || (every_core_is_stably_failing
                && (established_tcp_evidence || stable_timeout_evidence))
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
        // A ServerHello cannot override repeated failures while reading HTTP data.
        let confirmed_http_failure = core_hosts.iter().any(|host| {
            self.rounds
                .iter()
                .flat_map(|batch| &batch.targets)
                .filter(|target| {
                    target.core
                        && target.host == *host
                        && target.tls_ok
                        && target.failure_stage == FailureStage::Https
                })
                .count()
                >= required as usize
        });
        let eyes_working_ok = self.transport == StrategyTransport::Tls
            && self.category != AdaptiveCategory::Discord
            && !confirmed_http_failure
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
        let successful_rounds = round_passes(TargetProbeResult::final_ok).max(if eyes_working_ok {
            eyes_working_count
                .min(self.rounds.len() as u32)
                .min(u8::MAX as u32) as u8
        } else {
            0
        });
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
                path: crate::service_health::DISCORD_API_PATH,
                core: true,
            },
            ProbeTarget {
                host: "gateway.discord.gg",
                port: 443,
                path: "/",
                core: true,
            },
            ProbeTarget {
                host: "updates.discord.com",
                port: 443,
                path: crate::service_health::DISCORD_UPDATE_PATH,
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

/// Probes unrelated HTTPS endpoints before treating Discord-only timeouts as
/// trustworthy recovery evidence. One successful endpoint is sufficient to
/// prove that DNS plus the general TCP/TLS/HTTPS path is alive; Discord itself
/// is intentionally absent from this set.
pub async fn run_tls_control_probe(timeout: Duration) -> ProbeSeries {
    let targets = [
        ProbeTarget {
            host: "example.com",
            port: 443,
            path: "/",
            core: true,
        },
        ProbeTarget {
            host: "www.microsoft.com",
            port: 443,
            path: "/",
            core: true,
        },
        ProbeTarget {
            host: "www.cloudflare.com",
            port: 443,
            path: "/cdn-cgi/trace",
            core: true,
        },
    ];
    let dns_cache = SessionDnsCache::new();
    let batch = run_probe_round(
        AdaptiveCategory::Discord,
        StrategyTransport::Tls,
        &targets,
        1,
        timeout,
        dns_cache.inner.clone(),
    )
    .await;
    ProbeSeries {
        category: AdaptiveCategory::Discord,
        transport: StrategyTransport::Tls,
        required_successes: 1,
        rounds: vec![batch],
    }
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
        match probe_quic(&addresses, target.host, target.path, remaining).await {
            Ok(evidence) => {
                result.quic_ok = true;
                result.http_status = Some(evidence.status);
                result.https_ok = (200..=499).contains(&evidence.status);
                result.failure_stage = if result.https_ok {
                    FailureStage::None
                } else {
                    FailureStage::Https
                };
                result.detail = evidence.detail("HTTP/3");
            }
            Err(error) => {
                result.quic_ok = error.established;
                result.failure_stage = if error.established {
                    FailureStage::Https
                } else {
                    FailureStage::Quic
                };
                result.detail = format!("{}: {}", result.failure_stage.as_str(), error.detail);
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
            if crate::service_health::discord_path(target.host).is_some() {
                match crate::service_health::read_response(
                    response,
                    budget.remaining().unwrap_or_default(),
                )
                .await
                {
                    Ok(bytes) => {
                        result.https_ok = true;
                        result.failure_stage = FailureStage::None;
                        result.detail =
                            format!("HTTPS {status}; validated service document ({bytes} bytes)");
                    }
                    Err(error) => {
                        result.failure_stage = FailureStage::Https;
                        result.detail = format!("HTTPS {status}; {error}");
                    }
                }
                return finish(result, started);
            }
            match super::http_probe::read_http_body(response, budget.deadline).await {
                Ok(evidence) => {
                    result.https_ok = (200..=499).contains(&status);
                    result.failure_stage = if result.https_ok {
                        FailureStage::None
                    } else {
                        FailureStage::Https
                    };
                    result.detail = evidence.detail("HTTPS");
                }
                Err(error) => {
                    result.failure_stage = FailureStage::Https;
                    result.detail = format!("HTTPS {status}; {error}");
                }
            }
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
    resolve_dns_with_retry(budget, || async {
        tokio::net::lookup_host((target.host, target.port))
            .await
            .map(|addresses| bounded_addresses(addresses))
            .map_err(|error| format!("dns: system resolver: {error}"))
    })
    .await
}

async fn resolve_dns_with_retry<F, Fut>(
    budget: ProbeBudget,
    mut lookup: F,
) -> Result<Vec<SocketAddr>, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<SocketAddr>, String>>,
{
    let mut last_error = "dns: no addresses".to_string();
    for attempt in 0..2 {
        let remaining = budget
            .remaining()
            .ok_or_else(|| "dns: target timeout".to_string())?;
        // The first resolver call must leave room for the documented retry.
        let attempt_budget = if attempt == 0 {
            remaining / 2
        } else {
            remaining
        };
        match tokio::time::timeout(attempt_budget, lookup()).await {
            Ok(Ok(addresses)) => {
                if !addresses.is_empty() {
                    return Ok(addresses);
                }
                last_error = "dns: no addresses".into();
            }
            Ok(Err(error)) => last_error = format!("dns: {error}"),
            Err(_) => last_error = "dns: system resolver timeout after retry".into(),
        }
    }
    Err(last_error)
}
fn classify_reqwest_error(error: &reqwest::Error) -> FailureStage {
    if error.is_body() || error.is_decode() || error.status().is_some() {
        return FailureStage::Https;
    }
    if error.is_connect() {
        let chain = reqwest_error_chain(error);
        if reqwest_error_has_rustls_source(error) || connect_error_chain_is_tls(&chain) {
            return FailureStage::Tls;
        }
        return FailureStage::Tcp;
    }
    FailureStage::Https
}

fn reqwest_error_has_rustls_source(error: &reqwest::Error) -> bool {
    let mut source = error.source();
    while let Some(error) = source {
        if error.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        source = error.source();
    }
    false
}

fn connect_error_chain_is_tls(chain: &str) -> bool {
    let chain = chain.to_ascii_lowercase();
    [
        "tls",
        "certificate",
        "handshake",
        "rustls",
        // rustls record-layer failures can be boxed by hyper without keeping
        // the concrete rustls::Error type. They still prove that TCP was
        // established and the failure happened while decoding TLS records.
        "invalidcontenttype",
        "invalid content type",
        "invalidmessage",
        "corrupt message",
    ]
    .iter()
    .any(|marker| chain.contains(marker))
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
    path: &str,
    timeout: Duration,
) -> Result<super::http_probe::HttpEvidence, super::http_probe::QuicFailure> {
    super::http_probe::probe_http3(addresses, server_name, path, Instant::now() + timeout).await
}
fn finish(mut result: TargetProbeResult, started: Instant) -> TargetProbeResult {
    result.latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_server_hello_cannot_override_repeated_http_body_failures() {
        let mut rounds = vec![
            batch(1, true, true),
            batch(2, true, true),
            batch(3, true, true),
        ];
        for round in &mut rounds {
            for target in round.targets.iter_mut().filter(|target| target.core) {
                target.https_ok = false;
                target.failure_stage = FailureStage::Https;
                target.detail = "body timeout after 16384 bytes".into();
            }
        }
        let series = ProbeSeries {
            category: AdaptiveCategory::YoutubeTwitch,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds,
        };
        let eyes = EyesProbeEvidence {
            working_by_host: BTreeMap::from([
                ("youtube.com".into(), 3),
                ("www.youtube.com".into(), 3),
            ]),
            ..Default::default()
        };
        let result = series.evaluate(&eyes);
        assert!(!result.is_success());
        assert!(!result.eyes_working_ok);
        assert_eq!(result.failure_stage, FailureStage::Https);
    }

    #[test]
    fn rustls_record_errors_are_classified_as_tls_connect_failures() {
        for detail in [
            "received corrupt message of type InvalidContentType",
            "invalid content type in TLS record",
            "rustls handshake failure",
        ] {
            assert!(connect_error_chain_is_tls(detail), "{detail}");
        }
        assert!(!connect_error_chain_is_tls(
            "tcp: connection refused (os error 10061)"
        ));
    }

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
        assert_eq!(discord.len(), 3);
        assert!(discord
            .iter()
            .any(|target| target.host == "discord.com" && target.core));
        assert!(discord
            .iter()
            .any(|target| target.host == "gateway.discord.gg" && target.core));
        assert!(discord
            .iter()
            .any(|target| target.host == "updates.discord.com" && target.core));

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

        assert!(!result.is_success());
        assert!(!result.tls_or_quic_ok);
        assert!(!result.eyes_working_ok);
        assert!(!result.https_ok);
        assert_eq!(result.successful_rounds, 0);
        assert_eq!(result.failure_stage, FailureStage::Tls);
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
    fn discord_repeated_reset_and_timeout_is_reliable_tls_recovery_evidence() {
        let mut rounds = Vec::new();
        for round in 1..=3 {
            let mut batch = discord_failed_round(round, true);
            let discord = batch
                .targets
                .iter_mut()
                .find(|target| target.host == "discord.com")
                .unwrap();
            discord.tcp_ok = false;
            discord.failure_stage = FailureStage::Tcp;
            discord.detail = "operation timed out".into();
            let gateway = batch
                .targets
                .iter_mut()
                .find(|target| target.host == "gateway.discord.gg")
                .unwrap();
            gateway.tcp_ok = true;
            gateway.failure_stage = FailureStage::Tcp;
            gateway.detail = "connection reset (os error 10054)".into();
            rounds.push(batch);
        }
        let series = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds,
        };

        assert!(series.supports_tls_recovery());
    }

    #[test]
    fn discord_partial_core_outage_is_reliable_recovery_evidence() {
        let mut rounds = Vec::new();
        for round in 1..=3 {
            let healthy = TargetProbeResult {
                host: "gateway.discord.gg".into(),
                core: true,
                round,
                transport: StrategyTransport::Tls,
                dns_ok: true,
                tcp_ok: true,
                tls_ok: true,
                quic_ok: false,
                https_ok: true,
                http_status: Some(404),
                latency_ms: 200,
                failure_stage: FailureStage::None,
                detail: "HTTPS 404".into(),
            };
            let failed = TargetProbeResult {
                host: "updates.discord.com".into(),
                core: true,
                round,
                transport: StrategyTransport::Tls,
                dns_ok: true,
                tcp_ok: true,
                tls_ok: false,
                quic_ok: false,
                https_ok: false,
                http_status: None,
                latency_ms: 2_400,
                failure_stage: FailureStage::Tls,
                detail: "connection reset (os error 10054)".into(),
            };
            rounds.push(ProbeBatch {
                category: AdaptiveCategory::Discord,
                transport: StrategyTransport::Tls,
                round,
                targets: vec![healthy, failed],
            });
        }
        let series = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds,
        };

        assert!(series.supports_tls_recovery());
    }

    #[test]
    fn tls_recovery_rejects_dns_outage_and_timeout_only_environment() {
        let dns_outage = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds: vec![
                discord_failed_round(1, false),
                discord_failed_round(2, false),
                discord_failed_round(3, false),
            ],
        };
        assert!(!dns_outage.supports_tls_recovery());
        assert!(!dns_outage.supports_tls_recovery_with_control(true));

        let mut timeout_rounds = Vec::new();
        for round in 1..=3 {
            let mut batch = discord_failed_round(round, true);
            for target in &mut batch.targets {
                target.tcp_ok = false;
                target.failure_stage = FailureStage::Tcp;
                target.detail = "operation timed out".into();
            }
            timeout_rounds.push(batch);
        }
        let timeout_only = ProbeSeries {
            category: AdaptiveCategory::Discord,
            transport: StrategyTransport::Tls,
            required_successes: 2,
            rounds: timeout_rounds,
        };
        assert!(!timeout_only.supports_tls_recovery());
        assert!(timeout_only.supports_tls_recovery_with_control(true));

        let mut refused = timeout_only;
        for target in refused
            .rounds
            .iter_mut()
            .flat_map(|batch| batch.targets.iter_mut())
        {
            target.detail = "connection refused".into();
        }
        assert!(!refused.supports_tls_recovery_with_control(true));
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
    #[ignore = "Live diagnostic: makes bounded unauthenticated HTTPS requests"]
    async fn live_discord_probe_diagnostic() {
        let targets = targets_for(AdaptiveCategory::Discord, StrategyTransport::Tls);
        let series = run_probe_series_for_targets_with_cache(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            &targets,
            Duration::from_secs(5),
            2,
            2,
            Duration::from_millis(100),
            &SessionDnsCache::new(),
        )
        .await;
        for round in series.rounds {
            for result in round.targets {
                eprintln!("{}", serde_json::to_string(&result).unwrap());
            }
        }
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

    #[tokio::test]
    async fn stalled_first_dns_attempt_leaves_time_for_retry() {
        let mut attempts = 0;
        let addresses =
            resolve_dns_with_retry(ProbeBudget::new(Duration::from_millis(200)), || {
                attempts += 1;
                let attempt = attempts;
                async move {
                    if attempt == 1 {
                        std::future::pending::<()>().await;
                    }
                    Ok(vec!["127.0.0.1:443".parse().unwrap()])
                }
            })
            .await
            .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(addresses.len(), 1);
    }

    #[tokio::test]
    async fn dns_outage_remains_bounded_and_is_not_a_candidate_failure() {
        let start = Instant::now();
        let error = resolve_dns_with_retry(ProbeBudget::new(Duration::from_millis(100)), || {
            std::future::pending::<Result<Vec<SocketAddr>, String>>()
        })
        .await
        .unwrap_err();
        assert!(error.starts_with("dns:"));
        assert!(start.elapsed() < Duration::from_secs(1));
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
