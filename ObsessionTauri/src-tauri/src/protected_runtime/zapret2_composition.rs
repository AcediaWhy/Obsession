//! One bounded Discord composition. Donor evidence only schedules a new recipe;
//! it never becomes a successful measurement of that recipe.
use super::{
    zapret2_measure::{RoundMeasurement, TESTER_VERSION},
    zapret2_tournament::Tournament,
};
use obsession_runtime_protocol::zapret2_composition::{DiscordComposition, DiscordScope};

fn scope_passed(round: &RoundMeasurement, scope: DiscordScope) -> bool {
    if !round.comparable
        || round.runtime_generation.is_none()
        || round.transport != "tls"
        || round.tester_version != TESTER_VERSION
        || round.context_reason.is_some()
        || round.network_before.is_none()
        || round.network_before != round.network_after
    {
        return false;
    }
    let required: &[(&str, &str)] = match scope {
        DiscordScope::Api => &[
            ("api_large_tls", "discord.com"),
            ("api", "discord.com"),
            ("gateway", "gateway.discord.gg"),
            ("client_assets", "discord.com"),
        ],
        DiscordScope::Cdn => &[
            ("cdn_large_tls", "cdn.discordapp.com"),
            ("cdn", "cdn.discordapp.com"),
        ],
    };
    required.iter().all(|(function, host)| {
        let evidence: Vec<_> = round
            .functions
            .iter()
            .filter(|entry| entry.function == *function)
            .collect();
        evidence.len() == 1
            && evidence[0].required
            && evidence[0].passed()
            && evidence[0].routing_verified()
            && evidence[0].proof != "user_report"
            && reqwest::Url::parse(&evidence[0].target)
                .ok()
                .is_some_and(|url| {
                    url.host_str() == Some(*host) && matches!(url.scheme(), "https" | "wss")
                })
    })
}

fn repetitions(tournament: &Tournament, id: &str, scope: DiscordScope) -> usize {
    let rounds: Vec<_> = tournament
        .rounds
        .iter()
        .filter(|round| round.profile_id == id)
        .collect();
    if rounds.iter().all(|round| scope_passed(round, scope)) {
        rounds.len()
    } else {
        0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Action {
    Recheck(Vec<String>),
    Trial(DiscordComposition),
    None,
}

#[derive(Default)]
pub(super) struct Composer {
    pending: Option<DiscordComposition>,
    finished: bool,
}

impl Composer {
    pub fn next(&mut self, tournament: &mut Tournament, eligible: &[String]) -> Action {
        if self.finished {
            return Action::None;
        }
        if tournament
            .candidates
            .iter()
            .any(|candidate| candidate.family == "discord_composition")
        {
            return Action::None;
        }
        if let Some(pair) = self.pending.take() {
            self.finished = true;
            return if repetitions(tournament, &pair.api_profile_id, DiscordScope::Api) >= 2
                && repetitions(tournament, &pair.cdn_profile_id, DiscordScope::Cdn) >= 2
            {
                Action::Trial(pair)
            } else {
                Action::None
            };
        }
        // Prefer an existing complete recipe; composition is only a fallback
        // for observed complementary gaps, not a mandatory extra search.
        if !tournament.finalists(1).is_empty() {
            return Action::None;
        }
        for api in eligible {
            if repetitions(tournament, api, DiscordScope::Api) == 0 {
                continue;
            }
            for cdn in eligible {
                if api == cdn || repetitions(tournament, cdn, DiscordScope::Cdn) == 0 {
                    continue;
                }
                let Some(a) = tournament
                    .candidates
                    .iter()
                    .find(|candidate| &candidate.profile_id == api)
                else {
                    continue;
                };
                let Some(b) = tournament
                    .candidates
                    .iter()
                    .find(|candidate| &candidate.profile_id == cdn)
                else {
                    continue;
                };
                if a.fingerprint == b.fingerprint {
                    continue;
                }
                let Ok(pair) = DiscordComposition::new(api, cdn) else {
                    continue;
                };
                let recheck = [(api, DiscordScope::Api), (cdn, DiscordScope::Cdn)]
                    .into_iter()
                    .filter(|(id, scope)| repetitions(tournament, id, *scope) < 2)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                tournament.reserve_composition();
                if recheck.is_empty() {
                    self.finished = true;
                    return Action::Trial(pair);
                }
                self.pending = Some(pair);
                return Action::Recheck(recheck);
            }
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        zapret2_measure::Outcome,
        zapret2_tls::{Kind, Metadata},
        zapret2_tournament::{tests_round, Candidate},
    };
    use super::*;

    fn tournament() -> Tournament {
        Tournament::new(
            ["a", "b"]
                .into_iter()
                .map(|id| Candidate {
                    profile_id: id.into(),
                    label: id.into(),
                    family: id.into(),
                    fingerprint: id.into(),
                })
                .collect(),
            2,
            3,
        )
    }
    fn measured(id: &str, api: bool, cdn: bool) -> RoundMeasurement {
        let mut round = tests_round(id, true);
        round.network_before = Some("network".into());
        round.network_after = round.network_before.clone();
        let base = round.functions[0].clone();
        round.functions = [
            ("api_large_tls", "discord.com", api),
            ("api", "discord.com", api),
            ("gateway", "gateway.discord.gg", api),
            ("client_assets", "discord.com", api),
            ("cdn_large_tls", "cdn.discordapp.com", cdn),
            ("cdn", "cdn.discordapp.com", cdn),
        ]
        .into_iter()
        .map(|(function, host, passed)| {
            let mut row = base.clone();
            row.function = function.into();
            row.target = format!("https://{host}/");
            row.outcome = if passed {
                Outcome::Passed
            } else {
                Outcome::Failed
            };
            if function.ends_with("large_tls") {
                row.proof = "large_tls_application";
                row.tls_handshakes = [(Kind::LargeEarlySni, 200), (Kind::LargeLateSni, 1400)]
                    .into_iter()
                    .map(|(kind, offset)| Metadata {
                        kind,
                        handshake_bytes: 1800,
                        sni_offset: offset,
                        hybrid_key_bytes: 1216,
                        authenticated_tls: true,
                        application_ok: passed,
                    })
                    .collect();
            }
            row
        })
        .collect();
        round
    }
    fn record(t: &mut Tournament, mut round: RoundMeasurement) {
        round.ordinal = t.rounds.len() + 1;
        round.runtime_generation = Some(round.ordinal as u64);
        t.record(round).unwrap();
    }
    fn complementary() -> Tournament {
        let mut t = tournament();
        record(&mut t, measured("a", true, false));
        record(&mut t, measured("b", false, true));
        t
    }
    #[test]
    fn complementary_donors_need_repeats_and_combination_needs_its_own_full_rounds() {
        let mut t = complementary();
        let mut builder = Composer::default();
        let eligible = vec!["a".into(), "b".into()];
        assert_eq!(
            builder.next(&mut t, &eligible),
            Action::Recheck(eligible.clone())
        );
        assert_eq!(t.max_attempts(), 6);
        assert!(t.winner().is_none());
        record(&mut t, measured("b", false, true));
        record(&mut t, measured("a", true, false));
        let Action::Trial(pair) = builder.next(&mut t, &eligible) else {
            panic!("missing composition");
        };
        let id = pair.id();
        t.add_composition(Candidate {
            profile_id: id.clone(),
            label: "composed".into(),
            family: "discord_composition".into(),
            fingerprint: id.clone(),
        })
        .unwrap();
        assert_eq!(
            t.summary()
                .iter()
                .find(|row| row.profile_id == id)
                .unwrap()
                .rounds,
            0
        );
        let mut first = measured(&id, true, true);
        first.phase = "composition".into();
        record(&mut t, first);
        assert!(t.winner().is_none());
        record(&mut t, measured(&id, true, true));
        assert_eq!(t.winner(), Some(id.clone()));
        let summary = t
            .summary()
            .into_iter()
            .find(|row| row.profile_id == id)
            .unwrap();
        assert_eq!(summary.complete_rounds, 2);
        assert_eq!(summary.composition, Some(pair));
        assert_eq!(builder.next(&mut t, &eligible), Action::None);
        assert_eq!(t.max_attempts(), 6);
    }
    #[test]
    fn both_fail_or_shared_cdn_gap_cannot_create_a_recipe() {
        for outcomes in [
            [(false, false), (false, false)],
            [(true, false), (true, false)],
        ] {
            let mut t = tournament();
            for (id, (api, cdn)) in ["a", "b"].into_iter().zip(outcomes) {
                record(&mut t, measured(id, api, cdn));
            }
            assert_eq!(
                Composer::default().next(&mut t, &["a".into(), "b".into()]),
                Action::None
            );
            assert_eq!(t.max_attempts(), 3);
        }
    }
    #[test]
    fn inconsistent_recheck_discards_pair_and_failed_combination_is_not_confirmed() {
        let eligible = vec!["a".into(), "b".into()];
        for bad in [
            Outcome::Failed,
            Outcome::Inconclusive,
            Outcome::Cancelled,
            Outcome::NotTested,
        ] {
            let mut t = complementary();
            let mut builder = Composer::default();
            builder.next(&mut t, &eligible);
            let mut round = measured("a", true, false);
            round.functions[1].outcome = bad;
            record(&mut t, round);
            record(&mut t, measured("b", false, true));
            assert_eq!(builder.next(&mut t, &eligible), Action::None);
            assert!(t.winner().is_none());
        }
        let mut t = complementary();
        let mut builder = Composer::default();
        builder.next(&mut t, &eligible);
        record(&mut t, measured("a", true, false));
        record(&mut t, measured("b", false, true));
        let Action::Trial(pair) = builder.next(&mut t, &eligible) else {
            panic!();
        };
        let id = pair.id();
        t.add_composition(Candidate {
            profile_id: id.clone(),
            label: id.clone(),
            family: "discord_composition".into(),
            fingerprint: id.clone(),
        })
        .unwrap();
        record(&mut t, measured(&id, true, true));
        record(&mut t, measured(&id, true, false));
        assert!(t.winner().is_none());
        assert_eq!(
            t.summary()
                .iter()
                .find(|row| row.profile_id == id)
                .unwrap()
                .status,
            "partial"
        );
    }
    #[test]
    fn unknown_network_wrong_target_and_unproved_tls_do_not_supply_parts() {
        for mutation in 0..5 {
            let mut t = tournament();
            let mut round = measured("a", true, false);
            match mutation {
                0 => round.comparable = false,
                1 => round.network_after = None,
                2 => round.functions[1].target = "https://cdn.discordapp.com/".into(),
                3 => round.functions[0].tls_handshakes.clear(),
                _ => round.context_reason = Some("network_changed"),
            }
            record(&mut t, round);
            record(&mut t, measured("b", false, true));
            assert_eq!(
                Composer::default().next(&mut t, &["a".into(), "b".into()]),
                Action::None
            );
        }
    }
}
