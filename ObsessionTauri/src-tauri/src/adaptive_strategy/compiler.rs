//! Компиляция проверенного DSL в существующий `Zapret2Profile`.

use std::fmt;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::dsl::{StrategyCandidate, StrategyTransport, StrategyValue};
use super::validator::{self, ValidationIssue};
use crate::dpi_engine::zapret2::Zapret2Profile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    InvalidCandidate(Vec<ValidationIssue>),
    EmptyHostlist,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCandidate(issues) => write!(
                f,
                "adaptive candidate validation failed: {}",
                issues
                    .iter()
                    .map(|issue| issue.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Self::EmptyHostlist => write!(f, "adaptive candidate requires a hostlist"),
        }
    }
}

pub fn compile(
    candidate: &StrategyCandidate,
    hostlist: &Path,
) -> Result<Zapret2Profile, CompileError> {
    let report = validator::validate(candidate);
    if !report.is_valid() {
        return Err(CompileError::InvalidCandidate(report.errors));
    }
    if hostlist.as_os_str().is_empty() {
        return Err(CompileError::EmptyHostlist);
    }

    let lua_desync = candidate
        .steps
        .iter()
        .map(|step| {
            let mut rendered = step.function.as_lua_name().to_string();
            for (key, value) in &step.args {
                match value {
                    StrategyValue::Bool(true) => {
                        rendered.push(':');
                        rendered.push_str(key);
                    }
                    StrategyValue::Bool(false) => {
                        // Validator не допускает false-флаги; ветка оставлена как
                        // defensive fallback и не влияет на валидный DSL.
                    }
                    StrategyValue::Integer(number) => {
                        rendered.push(':');
                        rendered.push_str(key);
                        rendered.push('=');
                        rendered.push_str(&number.to_string());
                    }
                    StrategyValue::Text(text) => {
                        rendered.push(':');
                        rendered.push_str(key);
                        rendered.push('=');
                        rendered.push_str(text);
                    }
                }
            }
            rendered
        })
        .collect();

    let (filter_tcp, filter_udp, filter_l7) = match candidate.transport {
        StrategyTransport::Tls => (Some("443".into()), None, vec!["tls".into()]),
        StrategyTransport::Quic => (None, Some("443".into()), vec!["quic".into()]),
    };

    Ok(Zapret2Profile {
        name: candidate.candidate_id(),
        filter_tcp,
        filter_udp,
        filter_l7,
        hostlist: Some(hostlist.to_string_lossy().replace('\\', "/")),
        ipset: None,
        payload: candidate
            .payload
            .iter()
            .map(|payload| payload.as_name().to_string())
            .collect(),
        out_range: candidate
            .out_range
            .and_then(|range| range.as_cli().map(str::to_string)),
        in_range: None,
        lua_desync,
    })
}

/// Fingerprint семантики скомпилированного профиля. Имя candidate и абсолютный
/// hostlist намеренно исключены: разные DSL, дающие одинаковые effective argv,
/// должны расходовать одну попытку.
pub fn effective_fingerprint(candidate: &StrategyCandidate) -> String {
    let Ok(profile) = compile(candidate, Path::new("__adaptive_hostlist__")) else {
        return candidate.candidate_id();
    };
    let canonical = serde_json::json!({
        "filterTcp": profile.filter_tcp,
        "filterUdp": profile.filter_udp,
        "filterL7": profile.filter_l7,
        "payload": profile.payload,
        "outRange": profile.out_range,
        "inRange": profile.in_range,
        "luaDesync": profile.lua_desync,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let hash = Sha256::digest(bytes);
    let short = hash[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("effective-{short}")
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::adaptive_strategy::dsl::{
        AdaptiveCategory, AllowedPayload, AllowedRange, StrategyFunction, StrategyStep,
        StrategyValue,
    };
    use crate::dpi_engine::zapret2::{build_winws2_args, Zapret2Invocation};

    fn youtube_tls() -> StrategyCandidate {
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
    fn compiles_working_youtube_profile() {
        let candidate = youtube_tls();
        let profile = compile(&candidate, Path::new("C:\\lists\\youtube.txt")).unwrap();
        assert_eq!(profile.name, candidate.candidate_id());
        assert_eq!(profile.filter_tcp.as_deref(), Some("443"));
        assert_eq!(profile.filter_udp, None);
        assert_eq!(profile.filter_l7, ["tls"]);
        assert_eq!(profile.hostlist.as_deref(), Some("C:/lists/youtube.txt"));
        assert_eq!(profile.payload, ["tls_client_hello"]);
        assert_eq!(profile.out_range.as_deref(), Some("-d10"));
        assert_eq!(profile.lua_desync, ["multidisorder_legacy:pos=1,midsld"]);
    }

    #[test]
    fn renders_arguments_in_stable_key_order() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            vec![StrategyStep {
                function: StrategyFunction::Fake,
                args: BTreeMap::from([
                    ("tcp_ts_up".into(), StrategyValue::Bool(true)),
                    ("blob".into(), StrategyValue::Text("tls_google".into())),
                    ("repeats".into(), StrategyValue::Integer(4)),
                    ("tcp_ts".into(), StrategyValue::Integer(-30_000)),
                ]),
            }],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        );
        let profile = compile(&candidate, Path::new("discord.txt")).unwrap();
        assert_eq!(
            profile.lua_desync,
            ["fake:blob=tls_google:repeats=4:tcp_ts=-30000:tcp_ts_up"]
        );
    }

    #[test]
    fn compiles_quic_filter_without_tcp() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Quic,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_quic".into()))
                .with_arg("repeats", StrategyValue::Integer(6))],
            vec![AllowedPayload::QuicInitial],
            None,
        );
        let profile = compile(&candidate, Path::new("youtube.txt")).unwrap();
        assert_eq!(profile.filter_tcp, None);
        assert_eq!(profile.filter_udp.as_deref(), Some("443"));
        assert_eq!(profile.filter_l7, ["quic"]);
        assert_eq!(profile.out_range, None);
    }

    #[test]
    fn compiles_quic_ipfrag_chain() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Gaming,
            StrategyTransport::Quic,
            vec![
                StrategyStep::new(StrategyFunction::SendIpFrag)
                    .with_arg("ipfrag_pos_udp", StrategyValue::Integer(16)),
                StrategyStep::new(StrategyFunction::Drop),
            ],
            vec![AllowedPayload::QuicInitial],
            None,
        );
        let profile = compile(&candidate, Path::new("gaming-github.txt")).unwrap();
        assert_eq!(
            profile.lua_desync,
            ["send:ipfrag:ipfrag_pos_udp=16", "drop"]
        );
    }

    #[test]
    fn effective_fingerprint_ignores_nonsemantic_dsl_differences() {
        let mut explicit_always = youtube_tls();
        explicit_always.out_range = Some(AllowedRange::Always);
        let mut omitted = youtube_tls();
        omitted.out_range = None;

        assert_ne!(explicit_always.candidate_id(), omitted.candidate_id());
        assert_eq!(
            effective_fingerprint(&explicit_always),
            effective_fingerprint(&omitted)
        );
    }
    #[test]
    fn rejects_invalid_candidate_and_empty_hostlist() {
        let mut invalid = youtube_tls();
        invalid.steps.clear();
        assert!(matches!(
            compile(&invalid, Path::new("youtube.txt")),
            Err(CompileError::InvalidCandidate(_))
        ));
        assert_eq!(
            compile(&youtube_tls(), Path::new("")),
            Err(CompileError::EmptyHostlist)
        );
    }

    #[test]
    fn compiled_profiles_keep_builder_separator_invariant() {
        let first = compile(&youtube_tls(), Path::new("youtube.txt")).unwrap();
        let mut second_candidate = youtube_tls();
        second_candidate.steps[0]
            .args
            .insert("pos".into(), StrategyValue::Text("1,sniext".into()));
        let second = compile(&second_candidate, Path::new("youtube.txt")).unwrap();
        let args = build_winws2_args(&Zapret2Invocation {
            profiles: vec![first, second],
            ..Zapret2Invocation::default()
        });
        assert_eq!(args.iter().filter(|arg| arg.as_str() == "--new").count(), 1);
        assert_ne!(args.first().map(String::as_str), Some("--new"));
        assert_ne!(args.last().map(String::as_str), Some("--new"));
    }
}
