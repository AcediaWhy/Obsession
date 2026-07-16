//! Типизированный Safe Strategy DSL.
//!
//! Здесь нет raw Lua, CLI-строк и путей. Кандидат состоит только из enum-типов и
//! аргументов, которые дополнительно проверяет backend validator.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const STRATEGY_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    STRATEGY_SCHEMA_VERSION
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveCategory {
    Discord,
    YoutubeTwitch,
    Gaming,
}

impl AdaptiveCategory {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Discord => "discord",
            Self::YoutubeTwitch => "youtube_twitch",
            Self::Gaming => "gaming",
        }
    }

    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "discord" => Some(Self::Discord),
            "youtube" | "youtube_twitch" => Some(Self::YoutubeTwitch),
            "gaming" | "gaming_github" => Some(Self::Gaming),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyTransport {
    Tls,
    Quic,
}

impl StrategyTransport {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::Quic => "quic",
        }
    }
}

pub fn override_key(category: AdaptiveCategory, transport: StrategyTransport) -> String {
    format!("{}:{}", category.as_key(), transport.as_key())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrategyFunction {
    #[serde(rename = "fake")]
    Fake,
    #[serde(rename = "multisplit")]
    MultiSplit,
    #[serde(rename = "multidisorder")]
    MultiDisorder,
    #[serde(rename = "multidisorder_legacy")]
    MultiDisorderLegacy,
    #[serde(rename = "fakedsplit")]
    FakeDSplit,
    #[serde(rename = "fakeddisorder")]
    FakeDDisorder,
    #[serde(rename = "send_ipfrag")]
    SendIpFrag,
    #[serde(rename = "drop")]
    Drop,
}

impl StrategyFunction {
    pub const fn as_lua_name(self) -> &'static str {
        match self {
            Self::Fake => "fake",
            Self::MultiSplit => "multisplit",
            Self::MultiDisorder => "multidisorder",
            Self::MultiDisorderLegacy => "multidisorder_legacy",
            Self::FakeDSplit => "fakedsplit",
            Self::FakeDDisorder => "fakeddisorder",
            Self::SendIpFrag => "send:ipfrag",
            Self::Drop => "drop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StrategyValue {
    Bool(bool),
    Integer(i64),
    Text(String),
}

impl StrategyValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyStep {
    pub function: StrategyFunction,
    #[serde(default)]
    pub args: BTreeMap<String, StrategyValue>,
}

impl StrategyStep {
    pub fn new(function: StrategyFunction) -> Self {
        Self {
            function,
            args: BTreeMap::new(),
        }
    }

    pub fn with_arg(mut self, key: impl Into<String>, value: StrategyValue) -> Self {
        self.args.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AllowedPayload {
    #[serde(rename = "tls_client_hello")]
    TlsClientHello,
    #[serde(rename = "quic_initial")]
    QuicInitial,
}

impl AllowedPayload {
    pub const fn as_name(self) -> &'static str {
        match self {
            Self::TlsClientHello => "tls_client_hello",
            Self::QuicInitial => "quic_initial",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AllowedRange {
    #[serde(rename = "-d10")]
    FirstTenDataPackets,
    #[serde(rename = "always")]
    Always,
}

impl AllowedRange {
    pub const fn as_cli(self) -> Option<&'static str> {
        match self {
            Self::FirstTenDataPackets => Some("-d10"),
            Self::Always => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyCandidate {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub category: AdaptiveCategory,
    pub transport: StrategyTransport,
    pub steps: Vec<StrategyStep>,
    pub payload: Vec<AllowedPayload>,
    pub out_range: Option<AllowedRange>,
}

impl StrategyCandidate {
    pub fn new(
        category: AdaptiveCategory,
        transport: StrategyTransport,
        steps: Vec<StrategyStep>,
        payload: Vec<AllowedPayload>,
        out_range: Option<AllowedRange>,
    ) -> Self {
        Self {
            schema_version: STRATEGY_SCHEMA_VERSION,
            category,
            transport,
            steps,
            payload,
            out_range,
        }
    }

    /// Стабильный id нормализованного кандидата. `BTreeMap` гарантирует порядок
    /// аргументов, поэтому одинаковая стратегия имеет одинаковый id между запусками.
    pub fn candidate_id(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let hash = Sha256::digest(bytes);
        let short = hash[..12]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("adaptive-{short}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn youtube_candidate() -> StrategyCandidate {
        StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::MultiDisorderLegacy)
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        )
    }

    #[test]
    fn json_roundtrip_preserves_candidate() {
        let candidate = youtube_candidate();
        let json = serde_json::to_string(&candidate).unwrap();
        let decoded: StrategyCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, candidate);
    }

    #[test]
    fn candidate_id_is_stable_for_normalized_args() {
        let a = StrategyStep::new(StrategyFunction::Fake)
            .with_arg("repeats", StrategyValue::Integer(4))
            .with_arg("blob", StrategyValue::Text("tls_google".into()));
        let b = StrategyStep::new(StrategyFunction::Fake)
            .with_arg("blob", StrategyValue::Text("tls_google".into()))
            .with_arg("repeats", StrategyValue::Integer(4));
        let candidate_a = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            vec![a],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        );
        let candidate_b = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            vec![b],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        );
        assert_eq!(candidate_a.candidate_id(), candidate_b.candidate_id());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{
            "schema_version": 1,
            "category": "discord",
            "transport": "tls",
            "steps": [],
            "payload": ["tls_client_hello"],
            "out_range": "-d10",
            "raw_lua": "os.execute('bad')"
        }"#;
        assert!(serde_json::from_str::<StrategyCandidate>(json).is_err());
    }

    #[test]
    fn enum_names_match_strategy_grammar() {
        assert_eq!(
            StrategyFunction::MultiDisorderLegacy.as_lua_name(),
            "multidisorder_legacy"
        );
        assert_eq!(AllowedPayload::QuicInitial.as_name(), "quic_initial");
        assert_eq!(AllowedRange::FirstTenDataPackets.as_cli(), Some("-d10"));
    }
}
