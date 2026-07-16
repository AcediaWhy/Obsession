//! Детерминированный bounded-генератор кандидатов.

use std::collections::HashSet;

use super::compiler::effective_fingerprint;
use super::dsl::{
    AdaptiveCategory, AllowedPayload, AllowedRange, StrategyCandidate, StrategyFunction,
    StrategyStep, StrategyTransport, StrategyValue,
};
use super::evidence::FailureStage;
use super::validator;

pub const MAX_CANDIDATES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptiveDiagnosis {
    Reset,
    Blackhole,
    TlsHandshake,
    QuicHandshake,
}

pub struct GeneratorInput<'a> {
    pub category: AdaptiveCategory,
    pub diagnosis: AdaptiveDiagnosis,
    /// Подтверждённая стратегия этой сети. Она получает наивысший приоритет,
    /// если не совпадает с уже неработающей текущей стратегией.
    pub network_confirmed: Option<&'a StrategyCandidate>,
    /// Текущая стратегия исключается из результата: recovery уже начался,
    /// следовательно повторять её бессмысленно.
    pub current: Option<&'a StrategyCandidate>,
    pub tried: &'a HashSet<String>,
}

pub fn generate(input: GeneratorInput<'_>) -> Vec<StrategyCandidate> {
    let mut out = Vec::new();
    let mut seen = input.tried.clone();
    if let Some(current) = input.current {
        seen.insert(effective_fingerprint(current));
    }
    for baseline in builtin_baseline_candidates(input.category) {
        seen.insert(effective_fingerprint(&baseline));
    }

    let mut push = |candidate: StrategyCandidate| {
        if out.len() >= MAX_CANDIDATES
            || candidate.category != input.category
            || !validator::validate(&candidate).is_valid()
        {
            return;
        }
        let fingerprint = effective_fingerprint(&candidate);
        if seen.insert(fingerprint) {
            out.push(candidate);
        }
    };

    if let Some(saved) = input.network_confirmed {
        push(saved.clone());
    }

    match (input.category, input.diagnosis) {
        (AdaptiveCategory::YoutubeTwitch, AdaptiveDiagnosis::QuicHandshake) => {
            for candidate in youtube_quic_seeds() {
                push(candidate);
            }
            for candidate in youtube_tls_seeds() {
                push(candidate);
            }
        }
        (AdaptiveCategory::YoutubeTwitch, _) => {
            for candidate in youtube_tls_seeds() {
                push(candidate);
            }
            for candidate in youtube_quic_seeds() {
                push(candidate);
            }
        }
        (AdaptiveCategory::Discord, _) => {
            for candidate in discord_tls_seeds() {
                push(candidate);
            }
        }
        (AdaptiveCategory::Gaming, AdaptiveDiagnosis::QuicHandshake) => {
            for candidate in gaming_quic_seeds() {
                push(candidate);
            }
            for candidate in gaming_tls_seeds() {
                push(candidate);
            }
        }
        (AdaptiveCategory::Gaming, _) => {
            for candidate in gaming_tls_seeds() {
                push(candidate);
            }
            for candidate in gaming_quic_seeds() {
                push(candidate);
            }
        }
    }

    out.truncate(MAX_CANDIDATES);
    out
}

/// Профили bundled pack 0.2.3 служат только baseline/rollback и никогда не
/// расходуют adaptive attempt как якобы новый результат.
pub fn builtin_baseline_candidates(category: AdaptiveCategory) -> Vec<StrategyCandidate> {
    match category {
        AdaptiveCategory::YoutubeTwitch => {
            vec![
                youtube_tls_seeds()[0].clone(),
                youtube_quic_seeds()[0].clone(),
            ]
        }
        AdaptiveCategory::Discord => vec![discord_tls_seeds()[0].clone()],
        AdaptiveCategory::Gaming => {
            vec![
                gaming_tls_seeds()[0].clone(),
                gaming_quic_seeds()[0].clone(),
            ]
        }
    }
}

/// Evidence меняет порядок оставшихся allowlisted мутаций. Меньший score
/// означает более релевантный следующий кандидат; одинаковый score сохраняет
/// исходный детерминированный порядок.
pub fn candidate_priority(candidate: &StrategyCandidate, failure: FailureStage) -> (u8, u8) {
    let transport_score = match failure {
        FailureStage::Quic => u8::from(candidate.transport != StrategyTransport::Quic),
        _ => u8::from(candidate.transport != StrategyTransport::Tls),
    };
    let has_fake = candidate.steps.iter().any(|step| {
        matches!(
            step.function,
            StrategyFunction::Fake | StrategyFunction::FakeDSplit | StrategyFunction::FakeDDisorder
        )
    });
    let mutation_score = match failure {
        FailureStage::EyesBlackhole | FailureStage::Stability => {
            u8::from(!has_fake && candidate.steps.len() == 1)
        }
        FailureStage::Tls | FailureStage::Tcp | FailureStage::EyesReset => {
            u8::from(has_fake || candidate.steps.len() > 1)
        }
        FailureStage::Https => u8::from(candidate.out_range.is_none()),
        FailureStage::Spawn => candidate.steps.len().min(u8::MAX as usize) as u8,
        _ => 0,
    };
    (transport_score, mutation_score)
}

fn tls_candidate(category: AdaptiveCategory, steps: Vec<StrategyStep>) -> StrategyCandidate {
    StrategyCandidate::new(
        category,
        StrategyTransport::Tls,
        steps,
        vec![AllowedPayload::TlsClientHello],
        Some(AllowedRange::FirstTenDataPackets),
    )
}

fn quic_candidate(category: AdaptiveCategory, repeats: i64, blob: &str) -> StrategyCandidate {
    StrategyCandidate::new(
        category,
        StrategyTransport::Quic,
        vec![StrategyStep::new(StrategyFunction::Fake)
            .with_arg("blob", StrategyValue::Text(blob.into()))
            .with_arg("repeats", StrategyValue::Integer(repeats))],
        vec![AllowedPayload::QuicInitial],
        None,
    )
}

fn ipfrag_step(position: i64) -> StrategyStep {
    StrategyStep::new(StrategyFunction::SendIpFrag)
        .with_arg("ipfrag_pos_udp", StrategyValue::Integer(position))
}

fn quic_ipfrag_candidate(category: AdaptiveCategory, position: i64) -> StrategyCandidate {
    StrategyCandidate::new(
        category,
        StrategyTransport::Quic,
        vec![
            ipfrag_step(position),
            StrategyStep::new(StrategyFunction::Drop),
        ],
        vec![AllowedPayload::QuicInitial],
        None,
    )
}

fn quic_fake_ipfrag_candidate(
    category: AdaptiveCategory,
    blob: &str,
    repeats: i64,
    position: i64,
) -> StrategyCandidate {
    StrategyCandidate::new(
        category,
        StrategyTransport::Quic,
        vec![
            StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text(blob.into()))
                .with_arg("repeats", StrategyValue::Integer(repeats)),
            ipfrag_step(position),
            StrategyStep::new(StrategyFunction::Drop),
        ],
        vec![AllowedPayload::QuicInitial],
        None,
    )
}
fn split_step(function: StrategyFunction, pos: &str) -> StrategyStep {
    StrategyStep::new(function).with_arg("pos", StrategyValue::Text(pos.into()))
}

fn youtube_tls_seeds() -> Vec<StrategyCandidate> {
    let category = AdaptiveCategory::YoutubeTwitch;
    vec![
        // Фактически подтверждённый на текущем провайдере профиль pack 0.2.3.
        tls_candidate(
            category,
            vec![split_step(
                StrategyFunction::MultiDisorderLegacy,
                "1,midsld",
            )],
        ),
        tls_candidate(
            category,
            vec![split_step(StrategyFunction::MultiSplit, "1,midsld")],
        ),
        tls_candidate(
            category,
            vec![split_step(StrategyFunction::MultiDisorder, "1,midsld")],
        ),
        tls_candidate(
            category,
            vec![split_step(
                StrategyFunction::MultiDisorderLegacy,
                "1,sniext",
            )],
        ),
        tls_candidate(
            category,
            vec![
                StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                    .with_arg("tcp_md5", StrategyValue::Bool(true))
                    .with_arg("tcp_seq", StrategyValue::Integer(-10_000)),
                split_step(StrategyFunction::MultiDisorder, "1,midsld"),
            ],
        ),
        tls_candidate(
            category,
            vec![
                StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("tls_google".into()))
                    .with_arg("tcp_ts", StrategyValue::Integer(-30_000))
                    .with_arg("tcp_ts_up", StrategyValue::Bool(true))
                    .with_arg("repeats", StrategyValue::Integer(4)),
                split_step(StrategyFunction::MultiSplit, "1"),
            ],
        ),
        tls_candidate(
            category,
            vec![split_step(
                StrategyFunction::MultiDisorderLegacy,
                "1,midsld,sniext",
            )],
        ),
        tls_candidate(
            category,
            vec![StrategyStep::new(StrategyFunction::FakeDDisorder)
                .with_arg("blob", StrategyValue::Text("tls_google".into()))
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))
                .with_arg("repeats", StrategyValue::Integer(4))],
        ),
    ]
}

fn youtube_quic_seeds() -> Vec<StrategyCandidate> {
    quic_seeds(AdaptiveCategory::YoutubeTwitch)
}

fn gaming_tls_seeds() -> Vec<StrategyCandidate> {
    let mut seeds = youtube_tls_seeds();
    for seed in &mut seeds {
        seed.category = AdaptiveCategory::Gaming;
    }
    seeds
}

fn gaming_quic_seeds() -> Vec<StrategyCandidate> {
    let category = AdaptiveCategory::Gaming;
    vec![
        quic_candidate(category, 6, "fake_default_quic"),
        quic_candidate(category, 1, "fake_default_quic"),
        quic_candidate(category, 2, "fake_default_quic"),
        quic_candidate(category, 1, "quic_google"),
        quic_candidate(category, 2, "quic_google"),
        quic_candidate(category, 5, "fake_default_quic"),
        quic_candidate(category, 5, "quic_google"),
        quic_ipfrag_candidate(category, 8),
        quic_ipfrag_candidate(category, 16),
        quic_fake_ipfrag_candidate(category, "fake_default_quic", 5, 8),
        quic_fake_ipfrag_candidate(category, "quic_google", 5, 8),
        quic_ipfrag_candidate(category, 32),
        quic_ipfrag_candidate(category, 64),
    ]
}

fn quic_seeds(category: AdaptiveCategory) -> Vec<StrategyCandidate> {
    vec![
        quic_candidate(category, 6, "fake_default_quic"),
        quic_candidate(category, 1, "fake_default_quic"),
        quic_candidate(category, 2, "fake_default_quic"),
        quic_candidate(category, 5, "fake_default_quic"),
        quic_candidate(category, 10, "fake_default_quic"),
        quic_candidate(category, 12, "fake_default_quic"),
        quic_candidate(category, 1, "quic_google"),
        quic_candidate(category, 2, "quic_google"),
        quic_candidate(category, 5, "quic_google"),
        quic_candidate(category, 8, "quic_google"),
        quic_candidate(category, 10, "quic_google"),
        quic_candidate(category, 11, "quic_google"),
        quic_candidate(category, 12, "quic_google"),
    ]
}

fn timestamp_fake(blob: &str, repeats: i64) -> StrategyStep {
    StrategyStep::new(StrategyFunction::Fake)
        .with_arg("blob", StrategyValue::Text(blob.into()))
        .with_arg("tcp_ts", StrategyValue::Integer(-30_000))
        .with_arg("tcp_ts_up", StrategyValue::Bool(true))
        .with_arg("repeats", StrategyValue::Integer(repeats))
}

fn discord_baseline_mutation(
    repeats: i64,
    split: StrategyFunction,
    pos: &str,
) -> StrategyCandidate {
    tls_candidate(
        AdaptiveCategory::Discord,
        vec![
            timestamp_fake("tls_google", repeats),
            split_step(split, pos),
        ],
    )
}

fn discord_tls_seeds() -> Vec<StrategyCandidate> {
    let category = AdaptiveCategory::Discord;
    vec![
        // Exact pack 0.2.3 baseline. generate() excludes its effective fingerprint.
        discord_baseline_mutation(4, StrategyFunction::MultiSplit, "1"),
        discord_baseline_mutation(2, StrategyFunction::MultiSplit, "1"),
        discord_baseline_mutation(6, StrategyFunction::MultiSplit, "1"),
        discord_baseline_mutation(4, StrategyFunction::MultiSplit, "2"),
        discord_baseline_mutation(4, StrategyFunction::MultiSplit, "1,midsld"),
        discord_baseline_mutation(4, StrategyFunction::MultiDisorder, "1,midsld"),
        discord_baseline_mutation(4, StrategyFunction::MultiDisorderLegacy, "1,midsld"),
        tls_candidate(
            category,
            vec![
                timestamp_fake("fake_default_tls", 4),
                split_step(StrategyFunction::MultiSplit, "1"),
            ],
        ),
        // Official blockcheck2 TLS seed.
        tls_candidate(
            category,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                .with_arg("tcp_ts", StrategyValue::Integer(-1_000))],
        ),
        // Official Zapret2 config.default TLS profile.
        tls_candidate(
            category,
            vec![
                StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                    .with_arg("tcp_md5", StrategyValue::Bool(true))
                    .with_arg("tcp_seq", StrategyValue::Integer(-10_000)),
                split_step(StrategyFunction::MultiDisorder, "1,midsld"),
            ],
        ),
        // Official Zapret2 docs variant with bounded repeats.
        tls_candidate(
            category,
            vec![
                StrategyStep::new(StrategyFunction::Fake)
                    .with_arg("blob", StrategyValue::Text("fake_default_tls".into()))
                    .with_arg("tcp_md5", StrategyValue::Bool(true))
                    .with_arg("tcp_seq", StrategyValue::Integer(-10_000))
                    .with_arg("repeats", StrategyValue::Integer(6)),
                split_step(StrategyFunction::MultiDisorder, "midsld"),
            ],
        ),
        tls_candidate(
            category,
            vec![StrategyStep::new(StrategyFunction::FakeDSplit)
                .with_arg("blob", StrategyValue::Text("tls_google".into()))
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))
                .with_arg("tcp_ts", StrategyValue::Integer(-30_000))
                .with_arg("tcp_ts_up", StrategyValue::Bool(true))
                .with_arg("repeats", StrategyValue::Integer(4))],
        ),
        tls_candidate(
            category,
            vec![StrategyStep::new(StrategyFunction::FakeDDisorder)
                .with_arg("blob", StrategyValue::Text("tls_google".into()))
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))
                .with_arg("tcp_ts", StrategyValue::Integer(-30_000))
                .with_arg("tcp_ts_up", StrategyValue::Bool(true))
                .with_arg("repeats", StrategyValue::Integer(4))],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_tried() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn youtube_generation_is_deterministic_bounded_unique_and_valid() {
        let tried = empty_tried();
        let make = || {
            generate(GeneratorInput {
                category: AdaptiveCategory::YoutubeTwitch,
                diagnosis: AdaptiveDiagnosis::Blackhole,
                network_confirmed: None,
                current: None,
                tried: &tried,
            })
        };
        let a = make();
        let b = make();
        let ids_a: Vec<_> = a.iter().map(StrategyCandidate::candidate_id).collect();
        let ids_b: Vec<_> = b.iter().map(StrategyCandidate::candidate_id).collect();
        assert_eq!(ids_a, ids_b);
        assert!(!a.is_empty());
        assert!(a.len() <= MAX_CANDIDATES);
        assert_eq!(ids_a.iter().collect::<HashSet<_>>().len(), ids_a.len());
        assert!(a
            .iter()
            .all(|candidate| validator::validate(candidate).is_valid()));
    }

    #[test]
    fn network_confirmed_candidate_is_first() {
        let tried = empty_tried();
        let saved = tls_candidate(
            AdaptiveCategory::YoutubeTwitch,
            vec![split_step(StrategyFunction::MultiSplit, "sniext")],
        );
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::YoutubeTwitch,
            diagnosis: AdaptiveDiagnosis::Reset,
            network_confirmed: Some(&saved),
            current: None,
            tried: &tried,
        });
        assert_eq!(out[0].candidate_id(), saved.candidate_id());
    }

    #[test]
    fn current_and_tried_candidates_are_excluded() {
        let current = youtube_tls_seeds()[0].clone();
        let tried_candidate = youtube_tls_seeds()[1].clone();
        let tried = HashSet::from([effective_fingerprint(&tried_candidate)]);
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::YoutubeTwitch,
            diagnosis: AdaptiveDiagnosis::TlsHandshake,
            network_confirmed: None,
            current: Some(&current),
            tried: &tried,
        });
        let ids: HashSet<_> = out.iter().map(StrategyCandidate::candidate_id).collect();
        assert!(!ids.contains(&current.candidate_id()));
        assert!(!ids.contains(&tried_candidate.candidate_id()));
    }

    #[test]
    fn quic_diagnosis_prioritizes_quic_candidates() {
        let tried = empty_tried();
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::YoutubeTwitch,
            diagnosis: AdaptiveDiagnosis::QuicHandshake,
            network_confirmed: None,
            current: None,
            tried: &tried,
        });
        assert_eq!(out.len(), MAX_CANDIDATES);
        assert!(out
            .iter()
            .all(|candidate| candidate.transport == StrategyTransport::Quic));
        let expected = [
            ("fake_default_quic", 1),
            ("fake_default_quic", 2),
            ("fake_default_quic", 5),
            ("fake_default_quic", 10),
            ("fake_default_quic", 12),
            ("quic_google", 1),
            ("quic_google", 2),
            ("quic_google", 5),
            ("quic_google", 8),
            ("quic_google", 10),
            ("quic_google", 11),
            ("quic_google", 12),
        ];
        for (candidate, (blob, repeats)) in out.iter().zip(expected) {
            assert_eq!(
                candidate.steps[0].args.get("blob"),
                Some(&StrategyValue::Text(blob.into()))
            );
            assert_eq!(
                candidate.steps[0].args.get("repeats"),
                Some(&StrategyValue::Integer(repeats))
            );
        }
    }

    #[test]
    fn gaming_quic_generator_fills_budget_with_control_candidates() {
        let tried = empty_tried();
        let candidates = generate(GeneratorInput {
            category: AdaptiveCategory::Gaming,
            diagnosis: AdaptiveDiagnosis::QuicHandshake,
            network_confirmed: None,
            current: None,
            tried: &tried,
        });
        assert_eq!(candidates.len(), MAX_CANDIDATES);
        assert!(candidates
            .iter()
            .all(|candidate| candidate.category == AdaptiveCategory::Gaming));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.transport == StrategyTransport::Quic));
        assert!(candidates.iter().any(|candidate| candidate
            .steps
            .iter()
            .any(|step| step.function == StrategyFunction::SendIpFrag)));
        assert!(candidates.iter().any(|candidate| {
            candidate.steps.len() == 3
                && candidate.steps[0].function == StrategyFunction::Fake
                && candidate.steps[1].function == StrategyFunction::SendIpFrag
                && candidate.steps[2].function == StrategyFunction::Drop
        }));
        assert_eq!(
            builtin_baseline_candidates(AdaptiveCategory::Gaming).len(),
            2
        );
    }
    #[test]
    fn discord_generator_never_emits_quic() {
        let tried = empty_tried();
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::Discord,
            diagnosis: AdaptiveDiagnosis::Blackhole,
            network_confirmed: None,
            current: None,
            tried: &tried,
        });
        assert!(!out.is_empty());
        assert!(out
            .iter()
            .all(|candidate| candidate.transport == StrategyTransport::Tls));
        assert!(out
            .iter()
            .all(|candidate| candidate.category.as_key() == "discord"));
    }
    #[test]
    fn discord_ladder_fills_budget_in_approved_order_and_excludes_baseline() {
        let tried = empty_tried();
        let seeds = discord_tls_seeds();
        assert_eq!(seeds.len(), MAX_CANDIDATES + 1);

        let baseline_fingerprint = effective_fingerprint(&seeds[0]);
        let expected = seeds
            .iter()
            .skip(1)
            .map(effective_fingerprint)
            .collect::<Vec<_>>();
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::Discord,
            diagnosis: AdaptiveDiagnosis::TlsHandshake,
            network_confirmed: None,
            current: None,
            tried: &tried,
        });
        let actual = out.iter().map(effective_fingerprint).collect::<Vec<_>>();

        assert_eq!(out.len(), MAX_CANDIDATES);
        assert_eq!(actual, expected);
        assert!(!actual.contains(&baseline_fingerprint));
        assert_eq!(actual.iter().collect::<HashSet<_>>().len(), actual.len());
        assert!(out
            .iter()
            .all(|candidate| validator::validate(candidate).is_valid()));

        assert_eq!(
            out[0].steps[0].args.get("repeats"),
            Some(&StrategyValue::Integer(2))
        );
        assert_eq!(
            out[1].steps[0].args.get("repeats"),
            Some(&StrategyValue::Integer(6))
        );
        assert_eq!(
            out[2].steps[1].args.get("pos"),
            Some(&StrategyValue::Text("2".into()))
        );
    }

    #[test]
    fn builtin_baseline_candidates_are_never_generated() {
        let tried = empty_tried();
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::YoutubeTwitch,
            diagnosis: AdaptiveDiagnosis::Blackhole,
            network_confirmed: None,
            current: None,
            tried: &tried,
        });
        let fingerprints: HashSet<_> = out.iter().map(effective_fingerprint).collect();
        for baseline in builtin_baseline_candidates(AdaptiveCategory::YoutubeTwitch) {
            assert!(!fingerprints.contains(&effective_fingerprint(&baseline)));
        }
        assert!(!out.is_empty());
    }

    #[test]
    fn evidence_changes_candidate_priority() {
        let simple = youtube_tls_seeds()[1].clone();
        let aggressive = youtube_tls_seeds()[4].clone();
        assert!(
            candidate_priority(&simple, FailureStage::EyesReset)
                < candidate_priority(&aggressive, FailureStage::EyesReset)
        );
        assert!(
            candidate_priority(&aggressive, FailureStage::EyesBlackhole)
                < candidate_priority(&simple, FailureStage::EyesBlackhole)
        );
    }

    #[test]
    fn invalid_saved_candidate_is_ignored() {
        let tried = empty_tried();
        let mut invalid = youtube_tls_seeds()[0].clone();
        invalid.steps.clear();
        let out = generate(GeneratorInput {
            category: AdaptiveCategory::YoutubeTwitch,
            diagnosis: AdaptiveDiagnosis::Reset,
            network_confirmed: Some(&invalid),
            current: None,
            tried: &tried,
        });
        assert!(out.iter().all(|candidate| candidate != &invalid));
    }
}
