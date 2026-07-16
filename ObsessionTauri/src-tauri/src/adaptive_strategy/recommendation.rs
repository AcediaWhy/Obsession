//! Детерминированные рекомендации из подтверждённых стратегий той же сети.

use super::cache::{AdaptiveStrategyCache, StrategyTrust};
use super::dsl::{AdaptiveCategory, StrategyCandidate, StrategyTransport};
use super::validator;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrategyRecommendation {
    pub candidate: StrategyCandidate,
    pub source_category: AdaptiveCategory,
    pub source_candidate_id: String,
    pub source_transport: StrategyTransport,
    pub source_scope_fingerprint: Option<String>,
    pub reason: String,
}

pub fn for_gaming<F>(
    cache: &AdaptiveStrategyCache,
    network_key: &str,
    transport: StrategyTransport,
    engine_version: &str,
    target_scope_fingerprint: Option<&str>,
    mut scope_for: F,
) -> Option<StrategyRecommendation>
where
    F: FnMut(AdaptiveCategory) -> Option<String>,
{
    if cache
        .confirmed_candidate_for_transport_scoped(
            network_key,
            AdaptiveCategory::Gaming,
            transport,
            engine_version,
            target_scope_fingerprint,
        )
        .is_some()
    {
        return None;
    }

    let source_categories: &[AdaptiveCategory] = match transport {
        StrategyTransport::Tls => &[AdaptiveCategory::Discord, AdaptiveCategory::YoutubeTwitch],
        StrategyTransport::Quic => &[AdaptiveCategory::YoutubeTwitch],
    };
    let mut sources = source_categories
        .iter()
        .filter_map(|category| {
            let scope_fingerprint = scope_for(*category)?;
            let entry = cache.entry_for_transport_scoped(
                network_key,
                *category,
                transport,
                engine_version,
                Some(&scope_fingerprint),
            )?;
            (entry.trust == StrategyTrust::Confirmed).then_some((
                *category,
                scope_fingerprint,
                entry,
            ))
        })
        .collect::<Vec<_>>();

    sources.sort_by(|left, right| {
        left.2
            .failure_count
            .cmp(&right.2.failure_count)
            .then_with(|| right.2.success_count.cmp(&left.2.success_count))
            .then_with(|| right.2.confirmed_at.cmp(&left.2.confirmed_at))
            .then_with(|| left.2.candidate_id.cmp(&right.2.candidate_id))
    });
    let (source_category, source_scope_fingerprint, source) = sources.into_iter().next()?;

    let mut candidate = source.candidate;
    candidate.category = AdaptiveCategory::Gaming;
    if candidate.transport != transport || !validator::validate(&candidate).is_valid() {
        return None;
    }

    let source_name = match source_category {
        AdaptiveCategory::Discord => "Discord",
        AdaptiveCategory::YoutubeTwitch => "YouTube",
        AdaptiveCategory::Gaming => return None,
    };
    Some(StrategyRecommendation {
        candidate,
        source_category,
        source_candidate_id: source.candidate_id,
        source_transport: transport,
        source_scope_fingerprint: Some(source_scope_fingerprint),
        reason: format!(
            "Рекомендовано по {source_name} {} в этой сети",
            transport.as_key().to_uppercase()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_strategy::cache::ProbeSummary;
    use crate::adaptive_strategy::dsl::{
        AllowedPayload, AllowedRange, StrategyFunction, StrategyStep, StrategyValue,
    };

    fn tls_candidate(category: AdaptiveCategory, pos: &str) -> StrategyCandidate {
        StrategyCandidate::new(
            category,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::MultiDisorderLegacy)
                .with_arg("pos", StrategyValue::Text(pos.into()))],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        )
    }

    fn quic_candidate(category: AdaptiveCategory) -> StrategyCandidate {
        StrategyCandidate::new(
            category,
            StrategyTransport::Quic,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_quic".into()))
                .with_arg("repeats", StrategyValue::Integer(2))],
            vec![AllowedPayload::QuicInitial],
            None,
        )
    }

    fn probe(at: u64) -> ProbeSummary {
        ProbeSummary {
            passed: 3,
            failed: 0,
            measured_at: at,
        }
    }

    #[test]
    fn discord_tls_can_recommend_gaming_tls_without_transferring_confirmation() {
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                tls_candidate(AdaptiveCategory::Discord, "1,midsld"),
                Some("discord-scope"),
                10,
                probe(10),
            )
            .unwrap();

        let recommendation = for_gaming(
            &cache,
            "net",
            StrategyTransport::Tls,
            "1.0.2",
            Some("gaming-scope"),
            |category| (category == AdaptiveCategory::Discord).then(|| "discord-scope".into()),
        )
        .unwrap();

        assert_eq!(recommendation.candidate.category, AdaptiveCategory::Gaming);
        assert_eq!(recommendation.source_category, AdaptiveCategory::Discord);
        assert!(cache
            .confirmed_candidate_for_transport(
                "net",
                AdaptiveCategory::Gaming,
                StrategyTransport::Tls,
                "1.0.2",
            )
            .is_none());
    }

    #[test]
    fn youtube_quic_can_recommend_gaming_quic() {
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                quic_candidate(AdaptiveCategory::YoutubeTwitch),
                Some("youtube-scope"),
                10,
                probe(10),
            )
            .unwrap();

        let recommendation = for_gaming(
            &cache,
            "net",
            StrategyTransport::Quic,
            "1.0.2",
            Some("gaming-scope"),
            |_| Some("youtube-scope".into()),
        )
        .unwrap();
        assert_eq!(recommendation.source_transport, StrategyTransport::Quic);
        assert_eq!(
            recommendation.source_category,
            AdaptiveCategory::YoutubeTwitch
        );
    }

    #[test]
    fn ranking_prefers_more_confirmed_successes_before_freshness() {
        let mut cache = AdaptiveStrategyCache::default();
        let discord = tls_candidate(AdaptiveCategory::Discord, "1,midsld");
        let youtube = tls_candidate(AdaptiveCategory::YoutubeTwitch, "1");
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                discord,
                Some("discord-scope"),
                20,
                probe(20),
            )
            .unwrap();
        for at in [10, 11] {
            cache
                .put_confirmed_scoped(
                    "net",
                    None,
                    "1.0.2",
                    youtube.clone(),
                    Some("youtube-scope"),
                    at,
                    probe(at),
                )
                .unwrap();
        }

        let recommendation = for_gaming(
            &cache,
            "net",
            StrategyTransport::Tls,
            "1.0.2",
            Some("gaming-scope"),
            |category| match category {
                AdaptiveCategory::Discord => Some("discord-scope".into()),
                AdaptiveCategory::YoutubeTwitch => Some("youtube-scope".into()),
                AdaptiveCategory::Gaming => None,
            },
        )
        .unwrap();

        assert_eq!(
            recommendation.source_category,
            AdaptiveCategory::YoutubeTwitch
        );
    }

    #[test]
    fn tls_evidence_never_recommends_quic() {
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed(
                "net",
                None,
                "1.0.2",
                tls_candidate(AdaptiveCategory::YoutubeTwitch, "1"),
                10,
                probe(10),
            )
            .unwrap();

        assert!(for_gaming(
            &cache,
            "net",
            StrategyTransport::Quic,
            "1.0.2",
            None,
            |_| Some("scope".into()),
        )
        .is_none());
    }
}
