//! Backend-валидатор Safe Strategy DSL.

use std::collections::HashSet;

use serde::Serialize;

use super::dsl::{
    AdaptiveCategory, AllowedPayload, StrategyCandidate, StrategyFunction, StrategyStep,
    StrategyTransport, StrategyValue, STRATEGY_SCHEMA_VERSION,
};

const MAX_STEPS: usize = 3;
const MAX_REPEATS: i64 = 12;
const MAX_OFFSET: i64 = 100_000;
const MAX_SEQOVL: i64 = 2_048;

const TLS_BLOBS: &[&str] = &["fake_default_tls", "tls_google"];
const QUIC_BLOBS: &[&str] = &["fake_default_quic", "quic_google"];
const TLS_MODS: &[&str] = &[
    "rnd",
    "rndsni",
    "dupsid",
    "rnd,dupsid",
    "rnd,rndsni,dupsid",
    "rnd,dupsid,sni=www.google.com",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub errors: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>) {
        self.errors.push(ValidationIssue {
            code,
            message: message.into(),
        });
    }
}

pub fn validate(candidate: &StrategyCandidate) -> ValidationReport {
    let mut report = ValidationReport::default();

    if candidate.schema_version != STRATEGY_SCHEMA_VERSION {
        report.error(
            "schema_version",
            format!(
                "unsupported strategy schema {} (expected {})",
                candidate.schema_version, STRATEGY_SCHEMA_VERSION
            ),
        );
    }
    if candidate.steps.is_empty() {
        report.error("empty_steps", "strategy must contain at least one step");
    }
    if candidate.steps.len() > MAX_STEPS {
        report.error(
            "too_many_steps",
            format!("strategy has more than {MAX_STEPS} steps"),
        );
    }
    if candidate.category == AdaptiveCategory::Discord
        && candidate.transport == StrategyTransport::Quic
    {
        report.error(
            "unsupported_scope",
            "Discord QUIC/media is outside the MVP scope",
        );
    }

    validate_payload(candidate, &mut report);
    for (index, step) in candidate.steps.iter().enumerate() {
        validate_step(candidate.transport, index, step, &mut report);
    }
    validate_ipfrag_pairs(candidate, &mut report);
    report
}

fn validate_ipfrag_pairs(candidate: &StrategyCandidate, report: &mut ValidationReport) {
    for (index, step) in candidate.steps.iter().enumerate() {
        match step.function {
            StrategyFunction::SendIpFrag
                if candidate.steps.get(index + 1).map(|next| next.function)
                    != Some(StrategyFunction::Drop) =>
            {
                report.error("ipfrag_drop", "send:ipfrag must be followed by drop");
            }
            StrategyFunction::Drop
                if index == 0
                    || candidate.steps[index - 1].function != StrategyFunction::SendIpFrag =>
            {
                report.error("drop_pair", "drop must follow send:ipfrag");
            }
            _ => {}
        }
    }
}
fn validate_payload(candidate: &StrategyCandidate, report: &mut ValidationReport) {
    if candidate.payload.is_empty() {
        report.error("empty_payload", "payload filter must not be empty");
        return;
    }
    let unique: HashSet<_> = candidate.payload.iter().copied().collect();
    if unique.len() != candidate.payload.len() {
        report.error("duplicate_payload", "payload filter contains duplicates");
    }

    let expected = match candidate.transport {
        StrategyTransport::Tls => AllowedPayload::TlsClientHello,
        StrategyTransport::Quic => AllowedPayload::QuicInitial,
    };
    if candidate.payload.len() != 1 || candidate.payload[0] != expected {
        report.error(
            "transport_payload",
            format!(
                "transport {:?} requires payload {}",
                candidate.transport,
                expected.as_name()
            ),
        );
    }
}

fn validate_step(
    transport: StrategyTransport,
    index: usize,
    step: &StrategyStep,
    report: &mut ValidationReport,
) {
    if transport == StrategyTransport::Quic
        && !matches!(
            step.function,
            StrategyFunction::Fake | StrategyFunction::SendIpFrag | StrategyFunction::Drop
        )
    {
        report.error(
            "quic_function",
            format!(
                "step {index}: QUIC permits only allowlisted functions, got {}",
                step.function.as_lua_name()
            ),
        );
    }

    let allowed = match step.function {
        StrategyFunction::Fake => &[
            "blob",
            "repeats",
            "tcp_md5",
            "tcp_seq",
            "tcp_ts",
            "tcp_ts_up",
            "tls_mod",
            "badsum",
        ][..],
        StrategyFunction::MultiSplit
        | StrategyFunction::MultiDisorder
        | StrategyFunction::MultiDisorderLegacy => &["pos", "seqovl", "nodrop"][..],
        StrategyFunction::FakeDSplit | StrategyFunction::FakeDDisorder => &[
            "pos",
            "blob",
            "repeats",
            "tcp_md5",
            "tcp_seq",
            "tcp_ts",
            "tcp_ts_up",
            "badsum",
        ][..],
        StrategyFunction::SendIpFrag => &["ipfrag_pos_udp"][..],
        StrategyFunction::Drop => &[][..],
    };

    for (key, value) in &step.args {
        if !allowed.contains(&key.as_str()) {
            report.error(
                "unknown_arg",
                format!(
                    "step {index}: argument {key:?} is not allowed for {}",
                    step.function.as_lua_name()
                ),
            );
            continue;
        }
        validate_arg(transport, index, key, value, report);
    }

    if matches!(
        step.function,
        StrategyFunction::MultiSplit
            | StrategyFunction::MultiDisorder
            | StrategyFunction::MultiDisorderLegacy
            | StrategyFunction::FakeDSplit
            | StrategyFunction::FakeDDisorder
    ) && !step.args.contains_key("pos")
    {
        report.error(
            "missing_pos",
            format!("step {index}: {} requires pos", step.function.as_lua_name()),
        );
    }
    if matches!(
        step.function,
        StrategyFunction::Fake | StrategyFunction::FakeDSplit | StrategyFunction::FakeDDisorder
    ) && !step.args.contains_key("blob")
    {
        report.error(
            "missing_blob",
            format!(
                "step {index}: {} requires an explicit blob",
                step.function.as_lua_name()
            ),
        );
    }
}

fn validate_arg(
    transport: StrategyTransport,
    index: usize,
    key: &str,
    value: &StrategyValue,
    report: &mut ValidationReport,
) {
    match key {
        "blob" => {
            let Some(blob) = value.as_text() else {
                report.error("arg_type", format!("step {index}: blob must be text"));
                return;
            };
            let allowed = match transport {
                StrategyTransport::Tls => TLS_BLOBS,
                StrategyTransport::Quic => QUIC_BLOBS,
            };
            if !allowed.contains(&blob) {
                report.error(
                    "blob_allowlist",
                    format!("step {index}: blob {blob:?} is not allowed"),
                );
            }
        }
        "repeats" => validate_integer_range(index, key, value, 1, MAX_REPEATS, report),
        "tcp_seq" | "tcp_ts" => {
            if transport != StrategyTransport::Tls {
                report.error(
                    "transport_arg",
                    format!("step {index}: {key} is valid only for TLS"),
                );
            }
            validate_integer_range(index, key, value, -MAX_OFFSET, MAX_OFFSET, report);
        }
        "seqovl" => validate_integer_range(index, key, value, 1, MAX_SEQOVL, report),
        "tcp_md5" | "tcp_ts_up" | "badsum" | "nodrop" => {
            match value.as_bool() {
                None => {
                    report.error("arg_type", format!("step {index}: {key} must be boolean"));
                }
                Some(false) => {
                    report.error(
                        "false_flag",
                        format!("step {index}: false flag {key} must be omitted"),
                    );
                }
                Some(true) => {}
            }
            if transport == StrategyTransport::Quic && matches!(key, "tcp_md5" | "tcp_ts_up") {
                report.error(
                    "transport_arg",
                    format!("step {index}: {key} is valid only for TLS"),
                );
            }
        }
        "tls_mod" => {
            let Some(modifier) = value.as_text() else {
                report.error("arg_type", format!("step {index}: tls_mod must be text"));
                return;
            };
            if transport != StrategyTransport::Tls || !TLS_MODS.contains(&modifier) {
                report.error(
                    "tls_mod_allowlist",
                    format!("step {index}: tls_mod {modifier:?} is not allowed"),
                );
            }
        }
        "pos" => {
            let Some(pos) = value.as_text() else {
                report.error("arg_type", format!("step {index}: pos must be text"));
                return;
            };
            if !valid_position_list(pos) {
                report.error(
                    "position_allowlist",
                    format!("step {index}: position list {pos:?} is not allowed"),
                );
            }
        }
        "ipfrag_pos_udp" => validate_ipfrag_position(index, transport, value, report),
        _ => {}
    }
}

fn validate_ipfrag_position(
    index: usize,
    transport: StrategyTransport,
    value: &StrategyValue,
    report: &mut ValidationReport,
) {
    if transport != StrategyTransport::Quic {
        report.error(
            "transport_arg",
            format!("step {index}: ipfrag_pos_udp is valid only for QUIC"),
        );
    }
    let Some(number) = value.as_integer() else {
        report.error(
            "arg_type",
            format!("step {index}: ipfrag_pos_udp must be an integer"),
        );
        return;
    };
    if !matches!(number, 8 | 16 | 32 | 64) {
        report.error(
            "ipfrag_position",
            format!("step {index}: ipfrag_pos_udp={number} is not allowlisted"),
        );
    }
}
fn validate_integer_range(
    index: usize,
    key: &str,
    value: &StrategyValue,
    min: i64,
    max: i64,
    report: &mut ValidationReport,
) {
    let Some(number) = value.as_integer() else {
        report.error(
            "arg_type",
            format!("step {index}: {key} must be an integer"),
        );
        return;
    };
    if !(min..=max).contains(&number) {
        report.error(
            "arg_range",
            format!("step {index}: {key}={number} is outside {min}..={max}"),
        );
    }
}

fn valid_position_list(value: &str) -> bool {
    let positions: Vec<_> = value.split(',').collect();
    !positions.is_empty()
        && positions.len() <= 5
        && positions.iter().all(|position| valid_position(position))
}

fn valid_position(value: &str) -> bool {
    const BASES: &[&str] = &["1", "2", "midsld", "sniext", "host", "endhost"];
    if BASES.contains(&value) {
        return true;
    }
    for base in ["midsld", "sniext", "host", "endhost"] {
        if let Some(offset) = value.strip_prefix(base) {
            let Some(offset) = offset
                .strip_prefix('+')
                .or_else(|| offset.strip_prefix('-'))
            else {
                continue;
            };
            if !offset.is_empty()
                && offset.chars().all(|ch| ch.is_ascii_digit())
                && offset.parse::<u8>().is_ok_and(|number| number <= 16)
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::adaptive_strategy::dsl::{AllowedRange, StrategyValue};

    fn valid_youtube_tls() -> StrategyCandidate {
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
    fn accepts_working_youtube_legacy_disorder() {
        assert!(validate(&valid_youtube_tls()).is_valid());
    }

    #[test]
    fn accepts_bounded_discord_fake_and_split() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Tls,
            vec![
                StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("tls_google".into()))
                    .with_arg("tcp_ts", StrategyValue::Integer(-30_000))
                    .with_arg("tcp_ts_up", StrategyValue::Bool(true))
                    .with_arg("repeats", StrategyValue::Integer(4)),
                StrategyStep::new(StrategyFunction::MultiSplit)
                    .with_arg("pos", StrategyValue::Text("1".into())),
            ],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        );
        assert!(validate(&candidate).is_valid());
    }

    #[test]
    fn accepts_bounded_quic_ipfrag_chain() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Gaming,
            StrategyTransport::Quic,
            vec![
                StrategyStep::new(StrategyFunction::SendIpFrag)
                    .with_arg("ipfrag_pos_udp", StrategyValue::Integer(8)),
                StrategyStep::new(StrategyFunction::Drop),
            ],
            vec![AllowedPayload::QuicInitial],
            None,
        );
        assert!(validate(&candidate).is_valid());

        let mut invalid = candidate.clone();
        invalid.steps[0]
            .args
            .insert("ipfrag_pos_udp".into(), StrategyValue::Integer(9));
        invalid.steps.pop();
        let report = validate(&invalid);
        assert!(report
            .errors
            .iter()
            .any(|error| error.code == "ipfrag_position"));
        assert!(report
            .errors
            .iter()
            .any(|error| error.code == "ipfrag_drop"));
    }

    #[test]
    fn rejects_discord_quic_and_incompatible_payload() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::Discord,
            StrategyTransport::Quic,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_quic".into()))],
            vec![AllowedPayload::TlsClientHello],
            None,
        );
        let report = validate(&candidate);
        assert!(report.errors.iter().any(|e| e.code == "unsupported_scope"));
        assert!(report.errors.iter().any(|e| e.code == "transport_payload"));
    }

    #[test]
    fn rejects_unknown_arg_and_blob_path() {
        let mut candidate = valid_youtube_tls();
        candidate.steps[0].args.insert(
            "lua".into(),
            StrategyValue::Text("os.execute('bad')".into()),
        );
        candidate.steps.push(
            StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("../../evil.bin".into())),
        );
        let report = validate(&candidate);
        assert!(report.errors.iter().any(|e| e.code == "unknown_arg"));
        assert!(report.errors.iter().any(|e| e.code == "blob_allowlist"));
    }

    #[test]
    fn rejects_unbounded_repeats_and_offsets() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                .with_arg("repeats", StrategyValue::Integer(99))
                .with_arg("tcp_seq", StrategyValue::Integer(1_000_000))],
            vec![AllowedPayload::TlsClientHello],
            None,
        );
        let report = validate(&candidate);
        assert_eq!(
            report
                .errors
                .iter()
                .filter(|e| e.code == "arg_range")
                .count(),
            2
        );
    }

    #[test]
    fn rejects_bad_positions_and_missing_required_args() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![
                StrategyStep::new(StrategyFunction::MultiSplit)
                    .with_arg("pos", StrategyValue::Text("midsld+999,../../x".into())),
                StrategyStep::new(StrategyFunction::Fake),
            ],
            vec![AllowedPayload::TlsClientHello],
            None,
        );
        let report = validate(&candidate);
        assert!(report.errors.iter().any(|e| e.code == "position_allowlist"));
        assert!(report.errors.iter().any(|e| e.code == "missing_blob"));
    }

    #[test]
    fn rejects_empty_or_too_long_strategy() {
        let mut empty = valid_youtube_tls();
        empty.steps.clear();
        assert!(validate(&empty)
            .errors
            .iter()
            .any(|e| e.code == "empty_steps"));

        let mut long = valid_youtube_tls();
        long.steps = (0..4)
            .map(|_| {
                StrategyStep::new(StrategyFunction::MultiSplit)
                    .with_arg("pos", StrategyValue::Text("1".into()))
            })
            .collect();
        assert!(validate(&long)
            .errors
            .iter()
            .any(|e| e.code == "too_many_steps"));
    }

    #[test]
    fn rejects_wrong_arg_types() {
        let mut candidate = valid_youtube_tls();
        candidate.steps = vec![StrategyStep {
            function: StrategyFunction::Fake,
            args: BTreeMap::from([
                ("blob".into(), StrategyValue::Integer(1)),
                ("repeats".into(), StrategyValue::Text("four".into())),
            ]),
        }];
        let report = validate(&candidate);
        assert_eq!(
            report
                .errors
                .iter()
                .filter(|e| e.code == "arg_type")
                .count(),
            2
        );
    }

    #[test]
    fn rejects_false_flags_instead_of_silently_compiling_them() {
        let candidate = StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                .with_arg("tcp_md5", StrategyValue::Bool(false))],
            vec![AllowedPayload::TlsClientHello],
            None,
        );
        assert!(validate(&candidate)
            .errors
            .iter()
            .any(|error| error.code == "false_flag"));
    }

    #[test]
    fn position_allowlist_accepts_small_offsets_only() {
        assert!(valid_position_list("1,midsld,sniext+1,host-2"));
        assert!(!valid_position_list("midsld+17"));
        assert!(!valid_position_list("host+abc"));
        assert!(!valid_position_list("1,2,midsld,sniext,host,endhost"));
    }
}
