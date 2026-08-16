//! Strict wire contract between the medium-integrity Obsession UI and the
//! privileged Windows runtime service.
//!
//! This crate deliberately contains no filesystem, process or Windows API
//! operations. The absence of arbitrary path/argument request types is a
//! security boundary, not an incomplete convenience API.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 3;
pub const RUNTIME_PIPE_NAME: &str = r"\\.\pipe\ObsessionRuntime.v1";
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_REQUEST_ID_BYTES: usize = 64;
pub const MAX_SELECTIONS: usize = 5;
pub const MAX_STRATEGY_ID_BYTES: usize = 128;
pub const MAX_ZAPRET2_OVERRIDES: usize = 6;
pub const MAX_RELIABILITY_LANES: usize = MAX_SELECTIONS;
pub const MAX_RELIABILITY_COUNTER: u64 = u32::MAX as u64;
pub const MIN_FIREWALL_LEASE_SECONDS: u16 = 30;
pub const MAX_FIREWALL_LEASE_SECONDS: u16 = 60 * 60;
pub const MAX_HOSTS_CHECK_AGE_SECONDS: u32 = 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    FrameTooShort,
    FrameTooLarge,
    FrameLengthMismatch,
    InvalidJson,
    UnsupportedVersion,
    InvalidRequestId,
    InvalidSelections,
    InvalidStrategyId,
    InvalidGeneration,
    InvalidZapret2Level,
    InvalidZapret2Overrides,
    InvalidLegacyRecoveryControls,
    InvalidLegacyRecoveryApproval,
    InvalidPort,
    InvalidFirewallLease,
    InvalidHostsCheck,
    InvalidResponse,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::FrameTooShort => "frame is shorter than its length prefix",
            Self::FrameTooLarge => "frame exceeds the protocol size limit",
            Self::FrameLengthMismatch => "frame length prefix does not match payload",
            Self::InvalidJson => "frame contains invalid or unsupported JSON",
            Self::UnsupportedVersion => "protocol version is unsupported",
            Self::InvalidRequestId => "request id is invalid",
            Self::InvalidSelections => "DPI selections are empty, duplicated, or too numerous",
            Self::InvalidStrategyId => "strategy id is invalid",
            Self::InvalidGeneration => "runtime generation must be non-zero",
            Self::InvalidZapret2Level => "Zapret2 level must be between 0 and 3",
            Self::InvalidZapret2Overrides => {
                "Zapret2 adaptive overrides are inconsistent or outside the typed allowlist"
            }
            Self::InvalidLegacyRecoveryControls => {
                "Legacy recovery controls are inconsistent or unbounded"
            }
            Self::InvalidLegacyRecoveryApproval => {
                "Legacy recovery approval must contain non-zero opaque tokens"
            }
            Self::InvalidPort => "port must be non-zero",
            Self::InvalidFirewallLease => "firewall lease is outside the allowed range",
            Self::InvalidHostsCheck => "hosts check cache age is outside the allowed range",
            Self::InvalidResponse => "response contains invalid bounded data",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestEnvelope {
    pub protocol_version: u16,
    pub request_id: String,
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request_id: impl Into<String>, request: Request) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            request,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;
        validate_request_id(&self.request_id)?;
        self.request.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Request {
    GetCapabilities,
    GetRuntimeSnapshot,
    /// Removes only the authenticated caller's obsolete per-user install.
    /// The service derives both fixed legacy roots from the caller token SID;
    /// no path, executable or command payload is accepted on the wire.
    LegacyCleanup,
    /// Atomically replaces the authenticated service's bounded Legacy
    /// recovery controls. No paths, process identities, config ids or raw
    /// arguments are accepted from the caller.
    SetLegacyRecoveryControls(LegacyRecoveryControlsRequest),
    /// Approves only a currently published service-owned Assisted proposal.
    /// The service reconstructs every fence and process/resource identity.
    ApproveLegacyRecovery(LegacyRecoveryApprovalRequest),
    DpiStart(DpiStartRequest),
    /// Generation-fenced replacement of an active protected Zapret2 runtime.
    /// The nested request remains fully typed; no path, Lua or argv fragment is
    /// accepted from the caller.
    DpiReplace(DpiReplaceRequest),
    DpiStop(DpiStopRequest),
    HostsInstall(HostsMutationRequest),
    HostsCheck(HostsCheckRequest),
    HostsUninstall,
    HostsRestoreLastKnownGood(HostsMutationRequest),
    FirewallOpenProxyLan(FirewallOpenProxyLanRequest),
    FirewallCloseProxyLan,
    SubscribeEvents,
}

impl Request {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::DpiStart(request) => request.validate(),
            Self::DpiReplace(request) => request.validate(),
            Self::DpiStop(request) => request.validate(),
            Self::SetLegacyRecoveryControls(request) => request.validate(),
            Self::ApproveLegacyRecovery(request) => request.validate(),
            Self::FirewallOpenProxyLan(request) => request.validate(),
            Self::HostsCheck(request) => request.validate(),
            Self::GetCapabilities
            | Self::GetRuntimeSnapshot
            | Self::LegacyCleanup
            | Self::HostsInstall(_)
            | Self::HostsUninstall
            | Self::HostsRestoreLastKnownGood(_)
            | Self::FirewallCloseProxyLan
            | Self::SubscribeEvents => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DpiEngine {
    Legacy,
    Zapret2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DpiCategory {
    Discord,
    YoutubeTwitch,
    Gaming,
    AtRisk,
    Universal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyRecoveryMode {
    #[default]
    ObserveOnly,
    Assisted,
    Automatic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryControlsRequest {
    pub mode: LegacyRecoveryMode,
    pub automatic_paused: bool,
    pub frozen_categories: Vec<DpiCategory>,
}

impl LegacyRecoveryControlsRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.frozen_categories.len() > MAX_RELIABILITY_LANES
            || (self.mode != LegacyRecoveryMode::Automatic && !self.automatic_paused)
            || self
                .frozen_categories
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != self.frozen_categories.len()
        {
            Err(ProtocolError::InvalidLegacyRecoveryControls)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryApprovalRequest {
    pub proposal_id: u64,
    pub attempt_id: u64,
}

impl LegacyRecoveryApprovalRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.proposal_id == 0 || self.attempt_id == 0 {
            Err(ProtocolError::InvalidLegacyRecoveryApproval)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryControlsSnapshot {
    pub control_generation: u64,
    pub mode: LegacyRecoveryMode,
    pub automatic_paused: bool,
    pub frozen_categories: Vec<DpiCategory>,
}

impl LegacyRecoveryControlsSnapshot {
    pub const fn observe_only() -> Self {
        Self {
            control_generation: 1,
            mode: LegacyRecoveryMode::ObserveOnly,
            automatic_paused: true,
            frozen_categories: Vec::new(),
        }
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        LegacyRecoveryControlsRequest {
            mode: self.mode,
            automatic_paused: self.automatic_paused,
            frozen_categories: self.frozen_categories.clone(),
        }
        .validate()
        .and_then(|()| {
            if self.control_generation == 0 {
                Err(ProtocolError::InvalidResponse)
            } else {
                Ok(())
            }
        })
        .map_err(|_| ProtocolError::InvalidResponse)
    }
}

impl Default for LegacyRecoveryControlsSnapshot {
    fn default() -> Self {
        Self::observe_only()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiSelection {
    pub category: DpiCategory,
    /// Identifier resolved by the service against its protected manifest.
    /// It is never interpreted as a path or raw command-line fragment.
    pub strategy_id: String,
}

pub const ZAPRET2_ADAPTIVE_SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zapret2AdaptiveTransport {
    Tls,
    Quic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zapret2AdaptiveFunction {
    Fake,
    MultiSplit,
    MultiDisorder,
    MultiDisorderLegacy,
    FakeDSplit,
    FakeDDisorder,
    SendIpFrag,
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Zapret2AdaptiveValue {
    Bool(bool),
    Integer(i64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Zapret2AdaptiveStep {
    pub function: Zapret2AdaptiveFunction,
    #[serde(default)]
    pub args: BTreeMap<String, Zapret2AdaptiveValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zapret2AdaptivePayload {
    TlsClientHello,
    QuicInitial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zapret2AdaptiveRange {
    FirstTenDataPackets,
    Always,
}

/// Bounded Zapret2 candidate DSL. Text values are not command fragments: the
/// protocol validator accepts them only for the finite blob/tls-mod/position
/// grammars below, and the service renders the final Lua arguments itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Zapret2AdaptiveOverride {
    pub schema_version: u8,
    pub category: DpiCategory,
    pub transport: Zapret2AdaptiveTransport,
    pub steps: Vec<Zapret2AdaptiveStep>,
    pub payload: Zapret2AdaptivePayload,
    pub out_range: Option<Zapret2AdaptiveRange>,
}

impl Zapret2AdaptiveOverride {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != ZAPRET2_ADAPTIVE_SCHEMA_VERSION
            || !matches!(
                self.category,
                DpiCategory::Discord | DpiCategory::YoutubeTwitch | DpiCategory::Gaming
            )
            || (self.category == DpiCategory::Discord
                && self.transport == Zapret2AdaptiveTransport::Quic)
            || self.steps.is_empty()
            || self.steps.len() > 3
            || self.payload
                != match self.transport {
                    Zapret2AdaptiveTransport::Tls => Zapret2AdaptivePayload::TlsClientHello,
                    Zapret2AdaptiveTransport::Quic => Zapret2AdaptivePayload::QuicInitial,
                }
        {
            return Err(ProtocolError::InvalidZapret2Overrides);
        }

        for (index, step) in self.steps.iter().enumerate() {
            validate_zapret2_step(self.transport, step)?;
            match step.function {
                Zapret2AdaptiveFunction::SendIpFrag
                    if self.steps.get(index + 1).map(|next| next.function)
                        != Some(Zapret2AdaptiveFunction::Drop) =>
                {
                    return Err(ProtocolError::InvalidZapret2Overrides);
                }
                Zapret2AdaptiveFunction::Drop
                    if index == 0
                        || self.steps[index - 1].function
                            != Zapret2AdaptiveFunction::SendIpFrag =>
                {
                    return Err(ProtocolError::InvalidZapret2Overrides);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn validate_zapret2_step(
    transport: Zapret2AdaptiveTransport,
    step: &Zapret2AdaptiveStep,
) -> Result<(), ProtocolError> {
    if step.args.len() > 8
        || (transport == Zapret2AdaptiveTransport::Quic
            && !matches!(
                step.function,
                Zapret2AdaptiveFunction::Fake
                    | Zapret2AdaptiveFunction::SendIpFrag
                    | Zapret2AdaptiveFunction::Drop
            ))
    {
        return Err(ProtocolError::InvalidZapret2Overrides);
    }

    let allowed: &[&str] = match step.function {
        Zapret2AdaptiveFunction::Fake => &[
            "blob",
            "repeats",
            "tcp_md5",
            "tcp_seq",
            "tcp_ts",
            "tcp_ts_up",
            "tls_mod",
            "badsum",
        ],
        Zapret2AdaptiveFunction::MultiSplit
        | Zapret2AdaptiveFunction::MultiDisorder
        | Zapret2AdaptiveFunction::MultiDisorderLegacy => &["pos", "seqovl", "nodrop"],
        Zapret2AdaptiveFunction::FakeDSplit | Zapret2AdaptiveFunction::FakeDDisorder => &[
            "pos",
            "blob",
            "repeats",
            "tcp_md5",
            "tcp_seq",
            "tcp_ts",
            "tcp_ts_up",
            "badsum",
        ],
        Zapret2AdaptiveFunction::SendIpFrag => &["ipfrag_pos_udp"],
        Zapret2AdaptiveFunction::Drop => &[],
    };
    if step.args.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ProtocolError::InvalidZapret2Overrides);
    }

    let requires_position = matches!(
        step.function,
        Zapret2AdaptiveFunction::MultiSplit
            | Zapret2AdaptiveFunction::MultiDisorder
            | Zapret2AdaptiveFunction::MultiDisorderLegacy
            | Zapret2AdaptiveFunction::FakeDSplit
            | Zapret2AdaptiveFunction::FakeDDisorder
    );
    let requires_blob = matches!(
        step.function,
        Zapret2AdaptiveFunction::Fake
            | Zapret2AdaptiveFunction::FakeDSplit
            | Zapret2AdaptiveFunction::FakeDDisorder
    );
    if (requires_position && !step.args.contains_key("pos"))
        || (requires_blob && !step.args.contains_key("blob"))
    {
        return Err(ProtocolError::InvalidZapret2Overrides);
    }

    for (key, value) in &step.args {
        let valid = match (key.as_str(), value) {
            ("blob", Zapret2AdaptiveValue::Text(value)) => match transport {
                Zapret2AdaptiveTransport::Tls => {
                    matches!(value.as_str(), "fake_default_tls" | "tls_google")
                }
                Zapret2AdaptiveTransport::Quic => {
                    matches!(value.as_str(), "fake_default_quic" | "quic_google")
                }
            },
            ("repeats", Zapret2AdaptiveValue::Integer(value)) => (1..=12).contains(value),
            ("tcp_seq" | "tcp_ts", Zapret2AdaptiveValue::Integer(value)) => {
                transport == Zapret2AdaptiveTransport::Tls && (-100_000..=100_000).contains(value)
            }
            ("seqovl", Zapret2AdaptiveValue::Integer(value)) => (1..=2_048).contains(value),
            ("tcp_md5" | "tcp_ts_up", Zapret2AdaptiveValue::Bool(true)) => {
                transport == Zapret2AdaptiveTransport::Tls
            }
            ("badsum" | "nodrop", Zapret2AdaptiveValue::Bool(true)) => true,
            ("tls_mod", Zapret2AdaptiveValue::Text(value)) => {
                transport == Zapret2AdaptiveTransport::Tls
                    && matches!(
                        value.as_str(),
                        "rnd"
                            | "rndsni"
                            | "dupsid"
                            | "rnd,dupsid"
                            | "rnd,rndsni,dupsid"
                            | "rnd,dupsid,sni=www.google.com"
                    )
            }
            ("pos", Zapret2AdaptiveValue::Text(value)) => valid_zapret2_position_list(value),
            ("ipfrag_pos_udp", Zapret2AdaptiveValue::Integer(value)) => {
                transport == Zapret2AdaptiveTransport::Quic && matches!(*value, 8 | 16 | 32 | 64)
            }
            _ => false,
        };
        if !valid {
            return Err(ProtocolError::InvalidZapret2Overrides);
        }
    }
    Ok(())
}

fn valid_zapret2_position_list(value: &str) -> bool {
    let positions = value.split(',').collect::<Vec<_>>();
    !positions.is_empty()
        && positions.len() <= 5
        && positions.into_iter().all(valid_zapret2_position)
}

fn valid_zapret2_position(value: &str) -> bool {
    if matches!(value, "1" | "2" | "midsld" | "sniext" | "host" | "endhost") {
        return true;
    }
    ["midsld", "sniext", "host", "endhost"]
        .into_iter()
        .any(|base| {
            value.strip_prefix(base).is_some_and(|offset| {
                let digits = offset
                    .strip_prefix('+')
                    .or_else(|| offset.strip_prefix('-'));
                digits.is_some_and(|digits| {
                    !digits.is_empty()
                        && digits.bytes().all(|byte| byte.is_ascii_digit())
                        && digits.parse::<u8>().is_ok_and(|number| number <= 16)
                })
            })
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiRuntimeOptions {
    pub zapret2_level: u8,
    pub legacy_reliability: bool,
    #[serde(default)]
    pub zapret2_overrides: Vec<Zapret2AdaptiveOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiStartRequest {
    pub engine: DpiEngine,
    pub selections: Vec<DpiSelection>,
    pub options: DpiRuntimeOptions,
}

impl DpiStartRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.options.zapret2_level > 3 {
            return Err(ProtocolError::InvalidZapret2Level);
        }
        validate_dpi_selections(&self.selections)?;
        if self.engine != DpiEngine::Zapret2 && !self.options.zapret2_overrides.is_empty() {
            return Err(ProtocolError::InvalidZapret2Overrides);
        }
        if self.options.zapret2_overrides.len() > MAX_ZAPRET2_OVERRIDES {
            return Err(ProtocolError::InvalidZapret2Overrides);
        }
        let selected = self
            .selections
            .iter()
            .map(|selection| selection.category)
            .collect::<BTreeSet<_>>();
        let mut override_keys = BTreeSet::new();
        for candidate in &self.options.zapret2_overrides {
            if !selected.contains(&candidate.category)
                || !override_keys.insert((candidate.category, candidate.transport))
            {
                return Err(ProtocolError::InvalidZapret2Overrides);
            }
            candidate.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiReplaceRequest {
    pub expected_generation: u64,
    pub runtime: DpiStartRequest,
}

impl DpiReplaceRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.expected_generation == 0 {
            return Err(ProtocolError::InvalidGeneration);
        }
        self.runtime.validate()
    }
}

fn validate_dpi_selections(selections: &[DpiSelection]) -> Result<(), ProtocolError> {
    if selections.is_empty() || selections.len() > MAX_SELECTIONS {
        return Err(ProtocolError::InvalidSelections);
    }

    let mut categories = BTreeSet::new();
    for selection in selections {
        if !categories.insert(selection.category) {
            return Err(ProtocolError::InvalidSelections);
        }
        validate_strategy_id(&selection.strategy_id)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiStopRequest {
    pub generation: u64,
}

impl DpiStopRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.generation == 0 {
            Err(ProtocolError::InvalidGeneration)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostsProvider {
    Malw,
    Geohide,
    Comss,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostsMutationRequest {
    pub provider: HostsProvider,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostsCheckRequest {
    pub max_age_seconds: u32,
}

impl HostsCheckRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.max_age_seconds <= MAX_HOSTS_CHECK_AGE_SECONDS {
            Ok(())
        } else {
            Err(ProtocolError::InvalidHostsCheck)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiService {
    Chatgpt,
    Claude,
    Gemini,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiRouteHealth {
    Working,
    Unavailable,
    Inconclusive,
    Unchecked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiRouteKind {
    Preferred,
    Fallback,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiRouteFailureReason {
    Timeout,
    Tls,
    Dns,
    RouteMissing,
    Offline,
    ExternalChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiServiceRouteHealth {
    pub service: AiService,
    pub health: AiRouteHealth,
    pub route: AiRouteKind,
    pub provider: Option<HostsProvider>,
    pub reason: Option<AiRouteFailureReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostsHealthSnapshot {
    pub preferred_provider: HostsProvider,
    pub installed: bool,
    pub checked_at_unix: Option<u64>,
    pub repair_recommended: bool,
    pub services: Vec<AiServiceRouteHealth>,
}

impl HostsHealthSnapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.services.len() != 3 {
            return Err(ProtocolError::InvalidResponse);
        }
        let mut services = BTreeSet::new();
        for service in &self.services {
            if !services.insert(service.service)
                || matches!(service.route, AiRouteKind::Direct) != service.provider.is_none()
                || (matches!(service.health, AiRouteHealth::Working)
                    && matches!(service.route, AiRouteKind::Direct))
            {
                return Err(ProtocolError::InvalidResponse);
            }
        }
        if services != BTreeSet::from([AiService::Chatgpt, AiService::Claude, AiService::Gemini]) {
            return Err(ProtocolError::InvalidResponse);
        }
        Ok(())
    }
}

/// Sanitized service-owned hosts state. The IPC contract never exposes the
/// system path, downloaded payload, backup filenames or hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostsRuntimeSnapshot {
    pub provider: HostsProvider,
    pub installed: bool,
    pub externally_modified: bool,
    pub rollback_available: bool,
    pub local_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FirewallOpenProxyLanRequest {
    pub port: u16,
    /// A bounded lease is renewed by the client while LAN publication remains
    /// active. Permanent user-triggered firewall rules are intentionally absent.
    pub lease_seconds: u16,
}

impl FirewallOpenProxyLanRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.port == 0 {
            return Err(ProtocolError::InvalidPort);
        }
        if !(MIN_FIREWALL_LEASE_SECONDS..=MAX_FIREWALL_LEASE_SECONDS).contains(&self.lease_seconds)
        {
            return Err(ProtocolError::InvalidFirewallLease);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseEnvelope {
    pub protocol_version: u16,
    pub request_id: String,
    pub response: Response,
}

impl ResponseEnvelope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;
        validate_request_id(&self.request_id)?;
        match &self.response {
            Response::Capabilities(capabilities) => capabilities.validate(),
            Response::RuntimeSnapshot(snapshot) => snapshot.validate(),
            Response::HostsHealth(snapshot) => snapshot.validate(),
            Response::Accepted(accepted) if accepted.operation_id == 0 => {
                Err(ProtocolError::InvalidResponse)
            }
            Response::Started(started) if started.generation == 0 => {
                Err(ProtocolError::InvalidResponse)
            }
            Response::Accepted(_)
            | Response::Started(_)
            | Response::Stopped
            | Response::LegacyCleanupCompleted
            | Response::Error(_) => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Response {
    Capabilities(Capabilities),
    RuntimeSnapshot(RuntimeSnapshot),
    HostsHealth(HostsHealthSnapshot),
    Accepted(OperationAccepted),
    Started(RuntimeStarted),
    Stopped,
    LegacyCleanupCompleted,
    Error(ServiceError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    Dpi,
    /// The protected service can compile and launch the bundled Zapret2
    /// Strategy Pack without accepting caller-controlled argv or paths.
    DpiZapret2,
    /// The protected service supports generation-fenced replacement with the
    /// bounded typed Adaptive Zapret2 DSL.
    DpiZapret2Adaptive,
    EyesEvents,
    /// The service accepts bounded atomic Legacy mode/pause/freeze controls
    /// and publishes their service-owned generation. This does not imply that
    /// mutating recovery is enabled.
    LegacyReliabilityControls,
    /// The protected service exposes generation-fenced Legacy recovery
    /// operations in addition to read-only Eyes events.
    LegacyReliability,
    Hosts,
    HostsHealthV2,
    ProxyLanFirewall,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub service_version: String,
    pub features: Vec<Feature>,
}

impl Capabilities {
    fn validate(&self) -> Result<(), ProtocolError> {
        let version = self.service_version.as_bytes();
        if version.is_empty()
            || version.len() > 32
            || !version
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
        {
            return Err(ProtocolError::InvalidResponse);
        }
        if self.features.len() > 16 {
            return Err(ProtocolError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSnapshot {
    pub dpi: Option<DpiRuntimeSnapshot>,
    /// Sanitized service-owned observer state for the exact active DPI
    /// generation. Raw packets, hosts, paths, argv and process identities are
    /// deliberately absent from this contract.
    #[serde(default)]
    pub legacy_reliability: Option<LegacyReliabilityRuntimeSnapshot>,
    /// Sanitized process-presence signal produced by the protected service.
    /// Exact process identifiers, executable paths and socket owners never
    /// cross IPC; only target categories currently observed as running do.
    #[serde(default)]
    pub legacy_access_activity: Option<LegacyAccessActivitySnapshot>,
    /// Sanitized state of the protected hosts controller.
    #[serde(default)]
    pub hosts: Option<HostsRuntimeSnapshot>,
    /// Compatibility projection retained for older clients.
    pub hosts_provider: Option<HostsProvider>,
    pub proxy_lan: Option<ProxyLanLease>,
}

impl RuntimeSnapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(runtime) = &self.dpi {
            if runtime.generation == 0 || validate_dpi_selections(&runtime.selections).is_err() {
                return Err(ProtocolError::InvalidResponse);
            }
        }
        if let Some(reliability) = &self.legacy_reliability {
            let Some(dpi) = &self.dpi else {
                return Err(ProtocolError::InvalidResponse);
            };
            if reliability.generation != dpi.generation || reliability.validate().is_err() {
                return Err(ProtocolError::InvalidResponse);
            }
        }
        if self
            .legacy_access_activity
            .as_ref()
            .is_some_and(|activity| activity.validate().is_err())
        {
            return Err(ProtocolError::InvalidResponse);
        }
        if self
            .proxy_lan
            .as_ref()
            .is_some_and(|lease| lease.port == 0 || lease.expires_at_unix == 0)
        {
            return Err(ProtocolError::InvalidResponse);
        }
        if self.hosts.as_ref().is_some_and(|hosts| {
            hosts
                .local_version
                .as_deref()
                .is_some_and(|version| version.len() > 128)
        }) {
            return Err(ProtocolError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyAccessActivitySnapshot {
    /// Monotonic within one service process and advanced only when the
    /// sanitized running-category projection or sensor availability changes.
    pub revision: u64,
    pub sensor_available: bool,
    pub running_categories: Vec<DpiCategory>,
}

impl LegacyAccessActivitySnapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.revision == 0 || self.running_categories.len() > MAX_RELIABILITY_LANES {
            return Err(ProtocolError::InvalidResponse);
        }
        let unique = self
            .running_categories
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if unique.len() != self.running_categories.len()
            || (!self.sensor_available && !self.running_categories.is_empty())
        {
            Err(ProtocolError::InvalidResponse)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DpiRuntimeSnapshot {
    pub generation: u64,
    pub engine: DpiEngine,
    /// Service-verified catalog identities for the exact active generation.
    /// Paths, process identities and command-line fragments never cross IPC.
    pub selections: Vec<DpiSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyObserverHealth {
    Ready,
    Degraded,
    Blind,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyLanePhase {
    Observing,
    Healthy,
    Suspect,
    GatePending,
    BlockedCooldown,
    SensorUnreliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyAssessmentClassification {
    AwaitingEvidence,
    Working,
    Offline,
    DnsFailure,
    UpstreamDegraded,
    TargetUnavailable,
    ServiceSlow,
    DpiSuspected,
    DpiBlocked,
    SensorUnreliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyAssessmentConfidence {
    None,
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyEvidenceSnapshot {
    pub working_flows: u8,
    pub working_targets: u8,
    pub reset_flows: u8,
    pub reset_targets: u8,
    pub blackhole_flows: u8,
    pub blackhole_targets: u8,
}

impl LegacyEvidenceSnapshot {
    fn validate(self) -> Result<(), ProtocolError> {
        if self.working_flows > 2
            || self.working_targets > 2
            || self.reset_flows > 3
            || self.reset_targets > 2
            || self.blackhole_flows > 2
            || self.blackhole_targets > 2
        {
            Err(ProtocolError::InvalidResponse)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyLaneRuntimeSnapshot {
    pub category: DpiCategory,
    pub lane_generation: u64,
    pub phase: LegacyLanePhase,
    pub classification: LegacyAssessmentClassification,
    pub confidence: LegacyAssessmentConfidence,
    /// Transitional read-only compatibility for the short-lived service build
    /// that emitted this field under protocol v1. New services must never put
    /// it on the wire; new clients accept it until that build is retired.
    #[serde(default, skip_serializing)]
    pub working_confirmed_recently: bool,
    pub evidence: LegacyEvidenceSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyRecoveryPhase {
    Disabled,
    Idle,
    Evaluating,
    Executing,
    RollingBack,
    ManualIntervention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyRecoveryDisposition {
    ObserveOnly,
    Pending,
    Succeeded,
    RolledBack,
    Halted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRecoveryOrigin {
    Assisted,
    Automatic { control_generation: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryProposalSnapshot {
    pub proposal_id: u64,
    pub attempt_id: u64,
    pub incident_id: u64,
    pub category: DpiCategory,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub expires_at_monotonic_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRecoveryAttemptPhase {
    Preflight,
    Stopping,
    Starting,
    Confirming,
    RollingBack,
    Applied,
    ProcessFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryAttemptSnapshot {
    pub attempt_id: u64,
    pub incident_id: u64,
    pub category: DpiCategory,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub origin: LegacyRecoveryOrigin,
    pub phase: LegacyRecoveryAttemptPhase,
    pub phase_started_at_monotonic_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyRecoveryCompletionDisposition {
    CandidateApplied,
    PreviousPreserved,
    RolledBack,
    ProcessFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryCompletionSnapshot {
    pub attempt_id: u64,
    pub incident_id: u64,
    pub category: DpiCategory,
    pub previous_config_id: String,
    pub candidate_config_id: String,
    pub origin: LegacyRecoveryOrigin,
    pub phase: LegacyRecoveryAttemptPhase,
    pub disposition: LegacyRecoveryCompletionDisposition,
    pub finished_at_monotonic_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRecoveryRuntimeSnapshot {
    pub phase: LegacyRecoveryPhase,
    pub disposition: LegacyRecoveryDisposition,
    #[serde(default)]
    pub controls: LegacyRecoveryControlsSnapshot,
    #[serde(default)]
    pub proposal: Option<LegacyRecoveryProposalSnapshot>,
    #[serde(default)]
    pub active_attempt: Option<LegacyRecoveryAttemptSnapshot>,
    #[serde(default)]
    pub last_completion: Option<LegacyRecoveryCompletionSnapshot>,
    #[serde(default)]
    pub automatic_pacing_remaining_ms: Option<u64>,
    #[serde(default)]
    pub halted_categories: Vec<DpiCategory>,
    #[serde(default)]
    pub negative_cooldown_count: u16,
}

impl LegacyRecoveryRuntimeSnapshot {
    pub const fn observe_only() -> Self {
        Self {
            phase: LegacyRecoveryPhase::Disabled,
            disposition: LegacyRecoveryDisposition::ObserveOnly,
            controls: LegacyRecoveryControlsSnapshot::observe_only(),
            proposal: None,
            active_attempt: None,
            last_completion: None,
            automatic_pacing_remaining_ms: None,
            halted_categories: Vec::new(),
            negative_cooldown_count: 0,
        }
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        let valid = matches!(
            (self.phase, self.disposition),
            (
                LegacyRecoveryPhase::Disabled,
                LegacyRecoveryDisposition::ObserveOnly
            ) | (
                LegacyRecoveryPhase::Idle,
                LegacyRecoveryDisposition::ObserveOnly
                    | LegacyRecoveryDisposition::Succeeded
                    | LegacyRecoveryDisposition::RolledBack
            ) | (
                LegacyRecoveryPhase::Evaluating | LegacyRecoveryPhase::Executing,
                LegacyRecoveryDisposition::Pending
            ) | (
                LegacyRecoveryPhase::RollingBack,
                LegacyRecoveryDisposition::Pending
            ) | (
                LegacyRecoveryPhase::ManualIntervention,
                LegacyRecoveryDisposition::Halted
            )
        );
        let valid_categories = |categories: &[DpiCategory]| {
            categories.len() <= MAX_RELIABILITY_LANES
                && categories.iter().copied().collect::<BTreeSet<_>>().len() == categories.len()
        };
        let valid_id = |value: u64| value != 0;
        let valid_config = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_STRATEGY_ID_BYTES
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        };
        let valid_origin = |origin: LegacyRecoveryOrigin| match origin {
            LegacyRecoveryOrigin::Assisted => true,
            LegacyRecoveryOrigin::Automatic { control_generation } => control_generation != 0,
        };
        let proposal_valid = self.proposal.as_ref().is_none_or(|proposal| {
            valid_id(proposal.proposal_id)
                && valid_id(proposal.attempt_id)
                && valid_id(proposal.incident_id)
                && valid_config(&proposal.previous_config_id)
                && valid_config(&proposal.candidate_config_id)
                && proposal.previous_config_id != proposal.candidate_config_id
                && proposal.expires_at_monotonic_ms != 0
        });
        let attempt_valid = self.active_attempt.as_ref().is_none_or(|attempt| {
            valid_id(attempt.attempt_id)
                && valid_id(attempt.incident_id)
                && valid_config(&attempt.previous_config_id)
                && valid_config(&attempt.candidate_config_id)
                && attempt.previous_config_id != attempt.candidate_config_id
                && valid_origin(attempt.origin)
        });
        let completion_valid = self.last_completion.as_ref().is_none_or(|completion| {
            valid_id(completion.attempt_id)
                && valid_id(completion.incident_id)
                && valid_config(&completion.previous_config_id)
                && valid_config(&completion.candidate_config_id)
                && completion.previous_config_id != completion.candidate_config_id
                && valid_origin(completion.origin)
        });
        let lifecycle_valid = if self.proposal.is_some() {
            self.active_attempt.is_none()
                && self.controls.mode == LegacyRecoveryMode::Assisted
                && self.phase == LegacyRecoveryPhase::Evaluating
                && self.disposition == LegacyRecoveryDisposition::Pending
        } else if let Some(attempt) = self.active_attempt.as_ref() {
            self.disposition == LegacyRecoveryDisposition::Pending
                && self.phase
                    == match attempt.phase {
                        LegacyRecoveryAttemptPhase::Preflight => LegacyRecoveryPhase::Evaluating,
                        LegacyRecoveryAttemptPhase::RollingBack => LegacyRecoveryPhase::RollingBack,
                        LegacyRecoveryAttemptPhase::Stopping
                        | LegacyRecoveryAttemptPhase::Starting
                        | LegacyRecoveryAttemptPhase::Confirming
                        | LegacyRecoveryAttemptPhase::Applied
                        | LegacyRecoveryAttemptPhase::ProcessFailed => {
                            LegacyRecoveryPhase::Executing
                        }
                    }
        } else {
            !matches!(
                self.phase,
                LegacyRecoveryPhase::Evaluating
                    | LegacyRecoveryPhase::Executing
                    | LegacyRecoveryPhase::RollingBack
            )
        };
        if valid
            && self.controls.validate().is_ok()
            && valid_categories(&self.halted_categories)
            && (self.proposal.is_none() || self.active_attempt.is_none())
            && proposal_valid
            && attempt_valid
            && completion_valid
            && lifecycle_valid
        {
            Ok(())
        } else {
            Err(ProtocolError::InvalidResponse)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyReliabilityCounters {
    pub accepted_events: u64,
    pub rejected_events: u64,
    pub dropped_events: u64,
    pub packet_count: u64,
    pub parse_errors: u64,
    pub queue_drops: u64,
}

impl LegacyReliabilityCounters {
    fn validate(self) -> Result<(), ProtocolError> {
        let values = [
            self.accepted_events,
            self.rejected_events,
            self.dropped_events,
            self.packet_count,
            self.parse_errors,
            self.queue_drops,
        ];
        if values
            .into_iter()
            .all(|value| value <= MAX_RELIABILITY_COUNTER)
        {
            Ok(())
        } else {
            Err(ProtocolError::InvalidResponse)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyReliabilityRuntimeSnapshot {
    pub generation: u64,
    /// Monotonic within one service process and advanced whenever the
    /// service-owned observer publishes a new state.
    pub revision: u64,
    pub session_id: u64,
    pub sensor_generation: u64,
    pub registry_version: u64,
    pub health: LegacyObserverHealth,
    pub active_categories: Vec<DpiCategory>,
    pub lanes: Vec<LegacyLaneRuntimeSnapshot>,
    pub counters: LegacyReliabilityCounters,
    pub recovery: LegacyRecoveryRuntimeSnapshot,
}

impl LegacyReliabilityRuntimeSnapshot {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.generation == 0
            || self.revision == 0
            || self.session_id == 0
            || self.sensor_generation == 0
            || self.registry_version == 0
            || self.active_categories.is_empty()
            || self.active_categories.len() > MAX_RELIABILITY_LANES
            || self.lanes.len() != self.active_categories.len()
        {
            return Err(ProtocolError::InvalidResponse);
        }

        let active = self
            .active_categories
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if active.len() != self.active_categories.len() {
            return Err(ProtocolError::InvalidResponse);
        }
        let mut lanes = BTreeSet::new();
        for lane in &self.lanes {
            if lane.lane_generation == 0
                || !active.contains(&lane.category)
                || !lanes.insert(lane.category)
                || lane.evidence.validate().is_err()
            {
                return Err(ProtocolError::InvalidResponse);
            }
        }
        if lanes != active || self.counters.validate().is_err() || self.recovery.validate().is_err()
        {
            return Err(ProtocolError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProxyLanLease {
    pub port: u16,
    pub expires_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationAccepted {
    pub operation_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeStarted {
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceErrorCode {
    AccessDenied,
    Busy,
    Conflict,
    IncompatibleProtocol,
    InvalidRequest,
    ProtectedResourceInvalid,
    RuntimeFailed,
    ServiceUnavailable,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceError {
    pub code: ServiceErrorCode,
}

pub fn encode_request_frame(request: &RequestEnvelope) -> Result<Vec<u8>, ProtocolError> {
    request.validate()?;
    encode_json_frame(request)
}

pub fn decode_request_frame(frame: &[u8]) -> Result<RequestEnvelope, ProtocolError> {
    let request: RequestEnvelope = decode_json_frame(frame)?;
    request.validate()?;
    Ok(request)
}

pub fn encode_response_frame(response: &ResponseEnvelope) -> Result<Vec<u8>, ProtocolError> {
    response.validate()?;
    encode_json_frame(response)
}

pub fn decode_response_frame(frame: &[u8]) -> Result<ResponseEnvelope, ProtocolError> {
    let response: ResponseEnvelope = decode_json_frame(frame)?;
    response.validate()?;
    Ok(response)
}

fn encode_json_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let payload = serde_json::to_vec(value).map_err(|_| ProtocolError::InvalidJson)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_json_frame<T: for<'de> Deserialize<'de>>(frame: &[u8]) -> Result<T, ProtocolError> {
    if frame.len() < 4 {
        return Err(ProtocolError::FrameTooShort);
    }
    let declared = u32::from_le_bytes(frame[..4].try_into().expect("checked four bytes")) as usize;
    if declared > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    if frame.len() != declared + 4 {
        return Err(ProtocolError::FrameLengthMismatch);
    }
    serde_json::from_slice(&frame[4..]).map_err(|_| ProtocolError::InvalidJson)
}

fn validate_version(version: u16) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedVersion)
    }
}

fn validate_request_id(request_id: &str) -> Result<(), ProtocolError> {
    let bytes = request_id.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_REQUEST_ID_BYTES
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(ProtocolError::InvalidRequestId)
    } else {
        Ok(())
    }
}

fn validate_strategy_id(strategy_id: &str) -> Result<(), ProtocolError> {
    let bytes = strategy_id.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_STRATEGY_ID_BYTES
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Err(ProtocolError::InvalidStrategyId)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_start_request() -> RequestEnvelope {
        RequestEnvelope::new(
            "request-1",
            Request::DpiStart(DpiStartRequest {
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1".into(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: 0,
                    legacy_reliability: true,
                    zapret2_overrides: Vec::new(),
                },
            }),
        )
    }

    fn valid_adaptive_override() -> Zapret2AdaptiveOverride {
        Zapret2AdaptiveOverride {
            schema_version: ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
            category: DpiCategory::YoutubeTwitch,
            transport: Zapret2AdaptiveTransport::Tls,
            steps: vec![Zapret2AdaptiveStep {
                function: Zapret2AdaptiveFunction::MultiDisorderLegacy,
                args: BTreeMap::from([(
                    "pos".into(),
                    Zapret2AdaptiveValue::Text("1,midsld".into()),
                )]),
            }],
            payload: Zapret2AdaptivePayload::TlsClientHello,
            out_range: Some(Zapret2AdaptiveRange::FirstTenDataPackets),
        }
    }

    fn adaptive_start_request(overrides: Vec<Zapret2AdaptiveOverride>) -> DpiStartRequest {
        DpiStartRequest {
            engine: DpiEngine::Zapret2,
            selections: vec![DpiSelection {
                category: DpiCategory::YoutubeTwitch,
                strategy_id: "builtin-youtube-twitch".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 2,
                legacy_reliability: false,
                zapret2_overrides: overrides,
            },
        }
    }

    #[test]
    fn request_roundtrip_uses_a_bounded_length_prefixed_frame() {
        let request = valid_start_request();
        let frame = encode_request_frame(&request).unwrap();
        assert!(frame.len() < MAX_FRAME_BYTES);
        assert_eq!(decode_request_frame(&frame).unwrap(), request);
    }

    #[test]
    fn adaptive_replace_roundtrips_without_privileged_payloads() {
        let request = RequestEnvelope::new(
            "adaptive-replace-1",
            Request::DpiReplace(DpiReplaceRequest {
                expected_generation: 7,
                runtime: adaptive_start_request(vec![valid_adaptive_override()]),
            }),
        );
        let frame = encode_request_frame(&request).unwrap();
        assert_eq!(decode_request_frame(&frame).unwrap(), request);
        let json = String::from_utf8(frame[4..].to_vec()).unwrap();
        for forbidden in ["exePath", "configPath", "argv", "lua", "powershell"] {
            assert!(!json.contains(forbidden), "wire leaked {forbidden}: {json}");
        }
    }

    #[test]
    fn adaptive_dsl_rejects_paths_unknown_args_and_scope_confusion() {
        let mut path = valid_adaptive_override();
        path.steps[0] = Zapret2AdaptiveStep {
            function: Zapret2AdaptiveFunction::Fake,
            args: BTreeMap::from([(
                "blob".into(),
                Zapret2AdaptiveValue::Text(r"C:\Users\Public\payload.bin".into()),
            )]),
        };
        let mut unknown = valid_adaptive_override();
        unknown.steps[0].args.insert(
            "command".into(),
            Zapret2AdaptiveValue::Text("cmd /c whoami".into()),
        );
        let mut false_flag = valid_adaptive_override();
        false_flag.steps[0]
            .args
            .insert("nodrop".into(), Zapret2AdaptiveValue::Bool(false));
        let mut discord_quic = valid_adaptive_override();
        discord_quic.category = DpiCategory::Discord;
        discord_quic.transport = Zapret2AdaptiveTransport::Quic;
        discord_quic.payload = Zapret2AdaptivePayload::QuicInitial;

        for candidate in [path, unknown, false_flag, discord_quic] {
            assert_eq!(
                candidate.validate(),
                Err(ProtocolError::InvalidZapret2Overrides)
            );
        }

        let duplicate = valid_adaptive_override();
        let request = adaptive_start_request(vec![duplicate.clone(), duplicate]);
        assert_eq!(
            request.validate(),
            Err(ProtocolError::InvalidZapret2Overrides)
        );

        let mut wrong_category = valid_adaptive_override();
        wrong_category.category = DpiCategory::Gaming;
        assert_eq!(
            adaptive_start_request(vec![wrong_category]).validate(),
            Err(ProtocolError::InvalidZapret2Overrides)
        );

        let mut legacy = valid_start_request();
        let Request::DpiStart(start) = &mut legacy.request else {
            unreachable!();
        };
        start.options.zapret2_overrides = vec![valid_adaptive_override()];
        assert_eq!(
            encode_request_frame(&legacy),
            Err(ProtocolError::InvalidZapret2Overrides)
        );
    }

    #[test]
    fn adaptive_replace_requires_a_nonzero_generation() {
        let request = RequestEnvelope::new(
            "adaptive-replace-zero",
            Request::DpiReplace(DpiReplaceRequest {
                expected_generation: 0,
                runtime: adaptive_start_request(vec![valid_adaptive_override()]),
            }),
        );
        assert_eq!(
            encode_request_frame(&request),
            Err(ProtocolError::InvalidGeneration)
        );
    }

    #[test]
    fn legacy_recovery_controls_are_atomic_bounded_and_typed() {
        let request = RequestEnvelope::new(
            "legacy-controls-1",
            Request::SetLegacyRecoveryControls(LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::Automatic,
                automatic_paused: false,
                frozen_categories: vec![DpiCategory::Discord, DpiCategory::Gaming],
            }),
        );
        let frame = encode_request_frame(&request).unwrap();
        assert_eq!(decode_request_frame(&frame).unwrap(), request);
        let json = String::from_utf8(frame[4..].to_vec()).unwrap();
        assert!(!json.contains("path"));
        assert!(!json.contains("process"));
        assert!(!json.contains("strategy"));

        for invalid in [
            LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::Assisted,
                automatic_paused: false,
                frozen_categories: Vec::new(),
            },
            LegacyRecoveryControlsRequest {
                mode: LegacyRecoveryMode::Automatic,
                automatic_paused: true,
                frozen_categories: vec![DpiCategory::Discord, DpiCategory::Discord],
            },
        ] {
            assert_eq!(
                encode_request_frame(&RequestEnvelope::new(
                    "legacy-controls-invalid",
                    Request::SetLegacyRecoveryControls(invalid),
                )),
                Err(ProtocolError::InvalidLegacyRecoveryControls)
            );
        }
    }

    #[test]
    fn assisted_approval_contains_only_two_nonzero_opaque_tokens() {
        let request = RequestEnvelope::new(
            "legacy-approval-1",
            Request::ApproveLegacyRecovery(LegacyRecoveryApprovalRequest {
                proposal_id: 17,
                attempt_id: 19,
            }),
        );
        let frame = encode_request_frame(&request).unwrap();
        assert_eq!(decode_request_frame(&frame).unwrap(), request);
        let json = String::from_utf8(frame[4..].to_vec()).unwrap();
        assert!(!json.contains("category"));
        assert!(!json.contains("config"));
        assert!(!json.contains("path"));
        assert!(!json.contains("pid"));

        for (proposal_id, attempt_id) in [(0, 19), (17, 0), (0, 0)] {
            assert_eq!(
                encode_request_frame(&RequestEnvelope::new(
                    "legacy-approval-invalid",
                    Request::ApproveLegacyRecovery(LegacyRecoveryApprovalRequest {
                        proposal_id,
                        attempt_id,
                    }),
                )),
                Err(ProtocolError::InvalidLegacyRecoveryApproval)
            );
        }
    }

    #[test]
    fn unknown_envelope_and_payload_fields_are_rejected() {
        for json in [
            r#"{"protocolVersion":1,"requestId":"x","request":{"kind":"getCapabilities"},"exePath":"C:\\payload.exe"}"#,
            r#"{"protocolVersion":1,"requestId":"x","request":{"kind":"dpiStop","payload":{"generation":1,"command":"cmd /c whoami"}}}"#,
        ] {
            let mut frame = (json.len() as u32).to_le_bytes().to_vec();
            frame.extend_from_slice(json.as_bytes());
            assert_eq!(
                decode_request_frame(&frame),
                Err(ProtocolError::InvalidJson)
            );
        }
    }

    #[test]
    fn legacy_cleanup_is_payload_free_and_roundtrips_as_a_unit_operation() {
        let request = RequestEnvelope::new("cleanup-1", Request::LegacyCleanup);
        let frame = encode_request_frame(&request).unwrap();
        assert_eq!(decode_request_frame(&frame).unwrap(), request);

        let json = r#"{"protocolVersion":1,"requestId":"cleanup-2","request":{"kind":"legacyCleanup","payload":{"path":"C:\\Users\\Public"}}}"#;
        let mut frame = (json.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(json.as_bytes());
        assert_eq!(
            decode_request_frame(&frame),
            Err(ProtocolError::InvalidJson)
        );

        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "cleanup-1".into(),
            response: Response::LegacyCleanupCompleted,
        };
        let frame = encode_response_frame(&response).unwrap();
        assert_eq!(decode_response_frame(&frame).unwrap(), response);
    }

    #[test]
    fn arbitrary_paths_and_command_fragments_are_not_valid_strategy_ids() {
        for value in [
            r"C:\Users\Public\payload.exe",
            r"..\payload.dll",
            "powershell -EncodedCommand AAAA",
            "config.lua;cmd",
            "\\\\server\\share",
        ] {
            let mut request = valid_start_request();
            let Request::DpiStart(start) = &mut request.request else {
                unreachable!();
            };
            start.selections[0].strategy_id = value.into();
            assert_eq!(
                encode_request_frame(&request),
                Err(ProtocolError::InvalidStrategyId),
                "must reject {value:?}"
            );
        }
    }

    #[test]
    fn selections_are_nonempty_unique_and_bounded() {
        let mut request = valid_start_request();
        let Request::DpiStart(start) = &mut request.request else {
            unreachable!();
        };
        start.selections.clear();
        assert_eq!(
            encode_request_frame(&request),
            Err(ProtocolError::InvalidSelections)
        );

        let mut request = valid_start_request();
        let Request::DpiStart(start) = &mut request.request else {
            unreachable!();
        };
        start.selections.push(start.selections[0].clone());
        assert_eq!(
            encode_request_frame(&request),
            Err(ProtocolError::InvalidSelections)
        );
    }

    #[test]
    fn stop_and_firewall_requests_reject_unsafe_bounds() {
        let stop =
            RequestEnvelope::new("stop-1", Request::DpiStop(DpiStopRequest { generation: 0 }));
        assert_eq!(
            encode_request_frame(&stop),
            Err(ProtocolError::InvalidGeneration)
        );

        for request in [
            FirewallOpenProxyLanRequest {
                port: 0,
                lease_seconds: MIN_FIREWALL_LEASE_SECONDS,
            },
            FirewallOpenProxyLanRequest {
                port: 1443,
                lease_seconds: MIN_FIREWALL_LEASE_SECONDS - 1,
            },
            FirewallOpenProxyLanRequest {
                port: 1443,
                lease_seconds: MAX_FIREWALL_LEASE_SECONDS + 1,
            },
        ] {
            let envelope =
                RequestEnvelope::new("firewall-1", Request::FirewallOpenProxyLan(request));
            assert!(encode_request_frame(&envelope).is_err());
        }
    }

    #[test]
    fn hosts_health_contract_is_bounded_unique_and_consistent() {
        let services = vec![
            AiServiceRouteHealth {
                service: AiService::Chatgpt,
                health: AiRouteHealth::Working,
                route: AiRouteKind::Preferred,
                provider: Some(HostsProvider::Malw),
                reason: None,
            },
            AiServiceRouteHealth {
                service: AiService::Claude,
                health: AiRouteHealth::Working,
                route: AiRouteKind::Preferred,
                provider: Some(HostsProvider::Malw),
                reason: None,
            },
            AiServiceRouteHealth {
                service: AiService::Gemini,
                health: AiRouteHealth::Working,
                route: AiRouteKind::Fallback,
                provider: Some(HostsProvider::Geohide),
                reason: None,
            },
        ];
        let envelope = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "hosts-health".into(),
            response: Response::HostsHealth(HostsHealthSnapshot {
                preferred_provider: HostsProvider::Malw,
                installed: true,
                checked_at_unix: Some(1),
                repair_recommended: false,
                services: services.clone(),
            }),
        };
        assert!(envelope.validate().is_ok());

        let mut duplicate = services;
        duplicate[2].service = AiService::Claude;
        let invalid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "hosts-health-invalid".into(),
            response: Response::HostsHealth(HostsHealthSnapshot {
                preferred_provider: HostsProvider::Malw,
                installed: true,
                checked_at_unix: Some(1),
                repair_recommended: true,
                services: duplicate,
            }),
        };
        assert_eq!(invalid.validate(), Err(ProtocolError::InvalidResponse));

        let request = RequestEnvelope::new(
            "hosts-check-invalid",
            Request::HostsCheck(HostsCheckRequest {
                max_age_seconds: MAX_HOSTS_CHECK_AGE_SECONDS + 1,
            }),
        );
        assert_eq!(request.validate(), Err(ProtocolError::InvalidHostsCheck));
    }

    #[test]
    fn comss_route_provider_round_trips_in_camel_case_contract() {
        let encoded = serde_json::to_string(&HostsProvider::Comss).unwrap();
        assert_eq!(encoded, "\"comss\"");
        assert_eq!(
            serde_json::from_str::<HostsProvider>(&encoded).unwrap(),
            HostsProvider::Comss
        );
    }

    #[test]
    fn malformed_and_oversized_frames_fail_before_deserialization() {
        assert_eq!(decode_request_frame(&[]), Err(ProtocolError::FrameTooShort));

        let mut mismatch = 10u32.to_le_bytes().to_vec();
        mismatch.extend_from_slice(b"{}");
        assert_eq!(
            decode_request_frame(&mismatch),
            Err(ProtocolError::FrameLengthMismatch)
        );

        let oversized = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        assert_eq!(
            decode_request_frame(&oversized),
            Err(ProtocolError::FrameTooLarge)
        );
    }

    #[test]
    fn response_roundtrip_rejects_zero_runtime_identity() {
        let invalid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".into(),
            response: Response::Started(RuntimeStarted { generation: 0 }),
        };
        assert_eq!(
            encode_response_frame(&invalid),
            Err(ProtocolError::InvalidResponse)
        );

        let valid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".into(),
            response: Response::Started(RuntimeStarted { generation: 7 }),
        };
        let frame = encode_response_frame(&valid).unwrap();
        assert_eq!(decode_response_frame(&frame).unwrap(), valid);
    }

    #[test]
    fn engine_and_reliability_capabilities_roundtrip_without_raw_payloads() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "capabilities-1".into(),
            response: Response::Capabilities(Capabilities {
                service_version: "1.1.0".into(),
                features: vec![
                    Feature::Dpi,
                    Feature::DpiZapret2,
                    Feature::EyesEvents,
                    Feature::LegacyReliability,
                ],
            }),
        };

        let frame = encode_response_frame(&response).unwrap();
        assert_eq!(decode_response_frame(&frame).unwrap(), response);
    }

    #[test]
    fn runtime_snapshot_exposes_only_bounded_verified_selection_ids() {
        let valid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "snapshot-1".into(),
            response: Response::RuntimeSnapshot(RuntimeSnapshot {
                dpi: Some(DpiRuntimeSnapshot {
                    generation: 7,
                    engine: DpiEngine::Legacy,
                    selections: vec![DpiSelection {
                        category: DpiCategory::Discord,
                        strategy_id: "discord_1.conf".into(),
                    }],
                }),
                legacy_reliability: None,
                legacy_access_activity: None,
                hosts: None,
                hosts_provider: None,
                proxy_lan: None,
            }),
        };
        let frame = encode_response_frame(&valid).unwrap();
        assert_eq!(decode_response_frame(&frame).unwrap(), valid);

        let invalid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "snapshot-2".into(),
            response: Response::RuntimeSnapshot(RuntimeSnapshot {
                dpi: Some(DpiRuntimeSnapshot {
                    generation: 7,
                    engine: DpiEngine::Legacy,
                    selections: vec![DpiSelection {
                        category: DpiCategory::Discord,
                        strategy_id: "..\\payload.conf".into(),
                    }],
                }),
                legacy_reliability: None,
                legacy_access_activity: None,
                hosts: None,
                hosts_provider: None,
                proxy_lan: None,
            }),
        };
        assert_eq!(
            encode_response_frame(&invalid),
            Err(ProtocolError::InvalidResponse)
        );
    }

    #[test]
    fn reliability_snapshot_is_generation_fenced_bounded_and_sanitized() {
        let reliability = LegacyReliabilityRuntimeSnapshot {
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
                working_confirmed_recently: false,
                evidence: LegacyEvidenceSnapshot {
                    working_flows: 2,
                    working_targets: 2,
                    ..LegacyEvidenceSnapshot::default()
                },
            }],
            counters: LegacyReliabilityCounters {
                accepted_events: 23,
                packet_count: 29,
                ..LegacyReliabilityCounters::default()
            },
            recovery: LegacyRecoveryRuntimeSnapshot::observe_only(),
        };
        let valid = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "reliability-1".into(),
            response: Response::RuntimeSnapshot(RuntimeSnapshot {
                dpi: Some(DpiRuntimeSnapshot {
                    generation: 7,
                    engine: DpiEngine::Legacy,
                    selections: vec![DpiSelection {
                        category: DpiCategory::Discord,
                        strategy_id: "discord_1.conf".into(),
                    }],
                }),
                legacy_reliability: Some(reliability.clone()),
                legacy_access_activity: Some(LegacyAccessActivitySnapshot {
                    revision: 3,
                    sensor_available: true,
                    running_categories: vec![DpiCategory::Discord],
                }),
                hosts: None,
                hosts_provider: None,
                proxy_lan: None,
            }),
        };

        let frame = encode_response_frame(&valid).unwrap();
        assert_eq!(decode_response_frame(&frame).unwrap(), valid);
        let json = String::from_utf8(frame[4..].to_vec()).unwrap();
        assert!(!json.contains("workingConfirmedRecently"));
        let transitional_json = json.replacen(
            "\"confidence\":\"high\"",
            "\"confidence\":\"high\",\"workingConfirmedRecently\":true",
            1,
        );
        let transitional: ResponseEnvelope = serde_json::from_str(&transitional_json).unwrap();
        let Response::RuntimeSnapshot(transitional) = transitional.response else {
            unreachable!();
        };
        assert!(
            transitional
                .legacy_reliability
                .unwrap()
                .lanes
                .first()
                .unwrap()
                .working_confirmed_recently
        );
        for forbidden in ["path", "argv", "pid", "processIdentity", "rawPacket"] {
            assert!(!json.contains(forbidden));
        }

        let mut with_proposal = valid.clone();
        let Response::RuntimeSnapshot(snapshot) = &mut with_proposal.response else {
            unreachable!();
        };
        let recovery = &mut snapshot.legacy_reliability.as_mut().unwrap().recovery;
        recovery.phase = LegacyRecoveryPhase::Evaluating;
        recovery.disposition = LegacyRecoveryDisposition::Pending;
        recovery.controls = LegacyRecoveryControlsSnapshot {
            control_generation: 31,
            mode: LegacyRecoveryMode::Assisted,
            automatic_paused: true,
            frozen_categories: Vec::new(),
        };
        recovery.proposal = Some(LegacyRecoveryProposalSnapshot {
            proposal_id: 37,
            attempt_id: 41,
            incident_id: 43,
            category: DpiCategory::Discord,
            previous_config_id: "discord_1.conf".into(),
            candidate_config_id: "discord_2.conf".into(),
            expires_at_monotonic_ms: 47,
        });
        let proposal_frame = encode_response_frame(&with_proposal).unwrap();
        assert_eq!(
            decode_response_frame(&proposal_frame).unwrap(),
            with_proposal
        );

        let mut forged_proposal = with_proposal;
        let Response::RuntimeSnapshot(snapshot) = &mut forged_proposal.response else {
            unreachable!();
        };
        snapshot
            .legacy_reliability
            .as_mut()
            .unwrap()
            .recovery
            .proposal
            .as_mut()
            .unwrap()
            .candidate_config_id = "..\\payload.conf".into();
        assert_eq!(
            encode_response_frame(&forged_proposal),
            Err(ProtocolError::InvalidResponse)
        );

        let mut wrong_generation = valid.clone();
        let Response::RuntimeSnapshot(snapshot) = &mut wrong_generation.response else {
            unreachable!();
        };
        snapshot.legacy_reliability.as_mut().unwrap().generation = 8;
        assert_eq!(
            encode_response_frame(&wrong_generation),
            Err(ProtocolError::InvalidResponse)
        );

        let mut unbounded = valid.clone();
        let Response::RuntimeSnapshot(snapshot) = &mut unbounded.response else {
            unreachable!();
        };
        snapshot
            .legacy_reliability
            .as_mut()
            .unwrap()
            .counters
            .packet_count = MAX_RELIABILITY_COUNTER + 1;
        assert_eq!(
            encode_response_frame(&unbounded),
            Err(ProtocolError::InvalidResponse)
        );

        let mut invalid_controls = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "reliability-controls-invalid".into(),
            response: Response::RuntimeSnapshot(RuntimeSnapshot {
                dpi: Some(DpiRuntimeSnapshot {
                    generation: 7,
                    engine: DpiEngine::Legacy,
                    selections: vec![DpiSelection {
                        category: DpiCategory::Discord,
                        strategy_id: "discord_1.conf".into(),
                    }],
                }),
                legacy_reliability: Some(reliability),
                legacy_access_activity: None,
                hosts: None,
                hosts_provider: None,
                proxy_lan: None,
            }),
        };
        let Response::RuntimeSnapshot(snapshot) = &mut invalid_controls.response else {
            unreachable!();
        };
        snapshot
            .legacy_reliability
            .as_mut()
            .unwrap()
            .recovery
            .controls
            .control_generation = 0;
        assert_eq!(
            encode_response_frame(&invalid_controls),
            Err(ProtocolError::InvalidResponse)
        );

        let mut invalid_activity = valid.clone();
        let Response::RuntimeSnapshot(snapshot) = &mut invalid_activity.response else {
            unreachable!();
        };
        let activity = snapshot.legacy_access_activity.as_mut().unwrap();
        activity.running_categories.push(DpiCategory::Discord);
        assert_eq!(
            encode_response_frame(&invalid_activity),
            Err(ProtocolError::InvalidResponse)
        );
    }
}
