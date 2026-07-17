//! Типизированные причины результата adaptive probe.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SeriesVerdict {
    FinalSuccess,
    FinalFailure,
    Undecided,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    #[default]
    None,
    Dns,
    Tcp,
    Tls,
    Quic,
    Https,
    EyesReset,
    EyesBlackhole,
    Spawn,
    Stability,
}

impl FailureStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dns => "dns",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
            Self::Quic => "quic",
            Self::Https => "https",
            Self::EyesReset => "eyes_reset",
            Self::EyesBlackhole => "eyes_blackhole",
            Self::Spawn => "spawn",
            Self::Stability => "stability",
        }
    }
}
