//! Finite tournament policy. It owns evidence and scheduling, never a runtime.
#[cfg(test)]
use super::zapret2_measure::FunctionEvidence;
use super::zapret2_measure::{Outcome, RoundMeasurement};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet, VecDeque};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub profile_id: String,
    pub label: String,
    pub family: String,
    pub fingerprint: String,
}

/// Keep the baseline first, then try distinct mechanisms before siblings.
pub fn diverse(candidates: Vec<Candidate>, selected: Option<&str>, limit: usize) -> Vec<Candidate> {
    let mut entries = candidates;
    if let Some(position) = entries
        .iter()
        .position(|c| Some(c.profile_id.as_str()) == selected)
    {
        let baseline = entries.remove(position);
        entries.insert(0, baseline);
    }
    let mut unique = HashSet::new();
    entries.retain(|c| unique.insert(c.fingerprint.clone()));
    let mut result = Vec::new();
    let mut families = HashSet::new();
    let mut siblings = VecDeque::new();
    for entry in entries {
        if families.insert(entry.family.clone()) {
            result.push(entry);
        } else {
            siblings.push_back(entry);
        }
    }
    result.extend(siblings);
    result.truncate(limit);
    result
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionSummary {
    pub function: String,
    pub label: String,
    pub required: bool,
    pub passed: usize,
    pub failed: usize,
    pub inconclusive: usize,
    pub not_tested: usize,
    pub cancelled: usize,
    pub proof: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<obsession_runtime_protocol::zapret2_composition::DiscordComposition>,
    pub profile_id: String,
    pub label: String,
    pub family: String,
    pub fingerprint: String,
    pub rounds: usize,
    pub complete_rounds: usize,
    pub routing_verified_rounds: usize,
    pub required_confirmations: usize,
    pub status: &'static str,
    pub functions: Vec<FunctionSummary>,
    pub bytes: usize,
    pub median_ms: Option<u64>,
    pub transfer_bytes_per_sec: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NextStage {
    Screening(String),
    Confirmation(Vec<String>),
    Finished,
}

pub struct Tournament {
    pub candidates: Vec<Candidate>,
    pub rounds: Vec<RoundMeasurement>,
    required: usize,
    max_attempts: usize,
}

impl Tournament {
    pub fn new(candidates: Vec<Candidate>, required: usize, max_attempts: usize) -> Self {
        Self {
            candidates,
            rounds: Vec::new(),
            required,
            max_attempts,
        }
    }
    pub fn record(&mut self, round: RoundMeasurement) -> Result<(), &'static str> {
        if round.tester_version != super::zapret2_measure::TESTER_VERSION
            || !matches!(
                round.phase.as_str(),
                "screening" | "confirmation" | "composition"
            )
            || round
                .functions
                .iter()
                .any(|function| function.proof == "user_report")
            || self.rounds.len() >= self.max_attempts
            || round.ordinal != self.rounds.len() + 1
            || self
                .rounds
                .first()
                .is_some_and(|r| r.run_id != round.run_id)
            || (round.runtime_generation.is_some()
                && self
                    .rounds
                    .iter()
                    .any(|r| r.runtime_generation == round.runtime_generation))
            || !self
                .candidates
                .iter()
                .any(|c| c.profile_id == round.profile_id && c.fingerprint == round.fingerprint)
        {
            return Err("Измерение не соответствует текущему турниру.");
        }
        self.rounds.push(round);
        Ok(())
    }
    pub fn summary(&self) -> Vec<CandidateSummary> {
        let mut result = Vec::new();
        for candidate in &self.candidates {
            let rounds: Vec<_> = self
                .rounds
                .iter()
                .filter(|r| r.profile_id == candidate.profile_id)
                .collect();
            let complete = rounds.iter().filter(|r| r.complete()).count();
            let mut functions: BTreeMap<&str, FunctionSummary> = BTreeMap::new();
            for round in &rounds {
                for evidence in &round.functions {
                    let entry =
                        functions
                            .entry(&evidence.function)
                            .or_insert_with(|| FunctionSummary {
                                function: evidence.function.clone(),
                                label: evidence.label.clone(),
                                required: evidence.required,
                                passed: 0,
                                failed: 0,
                                inconclusive: 0,
                                not_tested: 0,
                                cancelled: 0,
                                proof: evidence.proof.into(),
                            });
                    // Invalidated context keeps raw outcomes in the round but
                    // cannot add positive or negative comparative evidence.
                    let outcome = if !round.comparable
                        && !matches!(evidence.outcome, Outcome::NotTested | Outcome::Cancelled)
                    {
                        Outcome::Inconclusive
                    } else {
                        evidence.outcome
                    };
                    match outcome {
                        Outcome::Passed => {
                            entry.passed += 1;
                            entry.proof = evidence.proof.into();
                        }
                        Outcome::Failed => entry.failed += 1,
                        Outcome::Inconclusive => entry.inconclusive += 1,
                        Outcome::NotTested => entry.not_tested += 1,
                        Outcome::Cancelled => entry.cancelled += 1,
                    }
                }
            }
            let status = if rounds.is_empty() {
                "not_tested"
            } else if functions.values().all(|f| f.not_tested == rounds.len()) {
                "not_tested"
            } else if complete >= self.required && complete == rounds.len() {
                "confirmed"
            } else if functions.values().any(|f| f.passed > 0) {
                "partial"
            } else if functions.values().any(|f| f.inconclusive > 0) {
                "inconclusive"
            } else {
                "failed"
            };
            let mut latencies: Vec<_> = rounds
                .iter()
                .filter(|r| r.complete())
                .map(|r| {
                    r.functions
                        .iter()
                        .filter(|f| f.required)
                        .map(|f| f.elapsed_ms)
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            latencies.sort_unstable();
            let mut rates: Vec<_> = rounds
                .iter()
                .filter(|r| r.complete())
                .filter_map(|r| {
                    r.functions
                        .iter()
                        .filter(|f| {
                            f.required
                                && f.proof == "http_transfer"
                                && f.transfer_bytes == super::zapret2_measure::TRANSFER_BYTES
                                && f.transfer_ms.is_some_and(|ms| ms > 0)
                        })
                        .map(|f| f.transfer_bytes as u64 * 1000 / f.transfer_ms.unwrap())
                        .min()
                })
                .collect();
            rates.sort_unstable();
            let transfer_bytes_per_sec =
                (rates.len() == complete && !rates.is_empty()).then(|| rates[rates.len() / 2]);
            result.push(CandidateSummary {
                composition:
                    obsession_runtime_protocol::zapret2_composition::DiscordComposition::parse(
                        &candidate.profile_id,
                    )
                    .ok()
                    .flatten(),
                profile_id: candidate.profile_id.clone(),
                label: candidate.label.clone(),
                family: candidate.family.clone(),
                fingerprint: candidate.fingerprint.clone(),
                rounds: rounds.len(),
                complete_rounds: complete,
                routing_verified_rounds: rounds
                    .iter()
                    .filter(|r| {
                        r.execution == "flow_routing_verified"
                            && r.functions
                                .iter()
                                .filter(|f| f.required)
                                .all(|f| f.routing_verified())
                    })
                    .count(),
                required_confirmations: self.required,
                status,
                functions: functions.into_values().collect(),
                bytes: rounds.iter().map(|r| r.bytes()).sum(),
                median_ms: (!latencies.is_empty()).then(|| latencies[latencies.len() / 2]),
                transfer_bytes_per_sec,
            });
        }
        // Lexicographic criteria, never a synthetic "probability". Similar
        // complete results remain a tie; milliseconds do not invent certainty.
        result.sort_by_key(|s| {
            (
                std::cmp::Reverse(s.status == "confirmed"),
                std::cmp::Reverse(s.complete_rounds),
                std::cmp::Reverse(
                    s.functions
                        .iter()
                        .filter(|f| f.required && f.passed > 0)
                        .count(),
                ),
                std::cmp::Reverse(s.transfer_bytes_per_sec.unwrap_or(0)),
            )
        });
        result
    }
    pub fn finalists(&self, count: usize) -> Vec<String> {
        self.summary()
            .into_iter()
            .filter(|s| s.complete_rounds > 0)
            .take(count)
            .map(|s| s.profile_id)
            .collect()
    }
    pub fn next_stage(&self, finalist_limit: usize) -> NextStage {
        if self.rounds.len() >= self.max_attempts {
            return NextStage::Finished;
        }
        let finalists = self.finalists(finalist_limit);
        if !finalists.is_empty() {
            return NextStage::Confirmation(finalists);
        }
        match self.candidates.iter().find(|candidate| {
            !self
                .rounds
                .iter()
                .any(|round| round.profile_id == candidate.profile_id)
        }) {
            Some(candidate) => NextStage::Screening(candidate.profile_id.clone()),
            None => NextStage::Finished,
        }
    }

    pub fn max_attempts(&self) -> usize {
        self.max_attempts
    }
    pub fn reserve_composition(&mut self) {
        self.max_attempts += 3;
    }
    pub fn add_composition(&mut self, candidate: Candidate) -> Result<(), &'static str> {
        if self.candidates.iter().any(|entry| {
            entry.profile_id == candidate.profile_id || entry.fingerprint == candidate.fingerprint
        }) || self
            .candidates
            .iter()
            .any(|entry| entry.family == "discord_composition")
        {
            return Err("Повторная сборка в одном турнире запрещена.");
        }
        self.candidates.push(candidate);
        Ok(())
    }

    pub fn winner(&self) -> Option<String> {
        let confirmed: Vec<_> = self
            .summary()
            .into_iter()
            .filter(|s| s.status == "confirmed")
            .collect();
        let first = confirmed.first()?;
        if confirmed.len() == 1 {
            return Some(first.profile_id.clone());
        }
        // Compare repeated equal-size transfers of the exact same public file.
        // A small latency difference or an unmeasured transport remains a tie.
        let signature = |id: &str| {
            let samples: Vec<_> = self
                .rounds
                .iter()
                .filter(|r| r.profile_id == id && r.complete())
                .flat_map(|r| {
                    r.functions
                        .iter()
                        .filter(|f| {
                            f.required
                                && f.proof == "http_transfer"
                                && f.transfer_bytes == super::zapret2_measure::TRANSFER_BYTES
                        })
                        .map(|f| (&f.function, &f.target))
                })
                .collect();
            let first = samples.first().copied()?;
            samples.iter().all(|s| *s == first).then_some(first)
        };
        let first_signature = signature(&first.profile_id)?;
        let speed_range = |id: &str| {
            let speeds: Vec<_> = self
                .rounds
                .iter()
                .filter(|r| r.profile_id == id && r.complete())
                .flat_map(|r| {
                    r.functions
                        .iter()
                        .filter(|f| {
                            f.required
                                && f.proof == "http_transfer"
                                && f.transfer_bytes == super::zapret2_measure::TRANSFER_BYTES
                                && f.transfer_ms.is_some_and(|ms| ms > 0)
                        })
                        .map(|f| f.transfer_bytes as u64 * 1000 / f.transfer_ms.unwrap())
                })
                .collect();
            Some((*speeds.iter().min()?, *speeds.iter().max()?))
        };
        let (fastest_min, _) = speed_range(&first.profile_id)?;
        if confirmed.iter().skip(1).all(|other| {
            signature(&other.profile_id) == Some(first_signature)
                && speed_range(&other.profile_id)
                    .is_some_and(|(_, other_max)| fastest_min as u128 * 2 >= other_max as u128 * 3)
        }) {
            Some(first.profile_id.clone())
        } else {
            None
        }
    }
    pub fn tied(&self) -> bool {
        self.winner().is_none()
            && self
                .summary()
                .iter()
                .filter(|s| s.status == "confirmed")
                .count()
                > 1
    }
}

pub fn confirmation_order(finalists: &[String], round: usize) -> Vec<String> {
    if finalists.is_empty() {
        return Vec::new();
    }
    let mut result = finalists.to_vec();
    let count = result.len();
    result.rotate_left((round + 1) % count);
    result
}

#[cfg(test)]
pub(super) fn tests_round(id: &str, passed: bool) -> RoundMeasurement {
    RoundMeasurement {
        run_id: "run".into(),
        profile_id: id.into(),
        fingerprint: id.into(),
        runtime_generation: Some(1),
        ordinal: 1,
        phase: "screening".into(),
        transport: "tls".into(),
        tester_version: super::zapret2_measure::TESTER_VERSION,
        execution: "flow_routing_verified",
        network_before: Some("network".into()),
        network_after: Some("network".into()),
        comparable: true,
        context_reason: None,
        packet_capture: None,
        functions: vec![FunctionEvidence {
            function: "api".into(),
            label: "API".into(),
            required: true,
            outcome: if passed {
                Outcome::Passed
            } else {
                Outcome::Failed
            },
            proof: "http_document",
            stage: "complete",
            reason: None,
            target: "https://test.invalid".into(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: 100,
            bytes: 100,
            transfer_bytes: 0,
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: Some(200),
            connections: vec![super::zapret2_routing::ConnectionEvidence {
                purpose: "unit_fixture".into(),
                host: "test.invalid".into(),
                local_address: "192.0.2.1:50001".parse().unwrap(),
                remote_address: "203.0.113.1:443".parse().unwrap(),
                expected_profile_id: id.into(),
                observed_profiles: vec![id.into()],
                observed_actions: vec!["fixture".into()],
                observed_sends: vec!["fixture".into()],
                status: "verified",
                http: None,
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_green_short_request_cannot_replace_both_large_authenticated_layouts() {
        use super::super::zapret2_tls::{Kind, Metadata};
        let mut round = tests_round("one", true);
        let function = &mut round.functions[0];
        function.function = "api_large_tls".into();
        assert!(!round.complete());
        round.functions[0].proof = "large_tls_application";
        round.functions[0].tls_handshakes = vec![
            Metadata {
                kind: Kind::LargeEarlySni,
                handshake_bytes: 1800,
                sni_offset: 150,
                hybrid_key_bytes: 1216,
                authenticated_tls: true,
                application_ok: true,
            },
            Metadata {
                kind: Kind::LargeLateSni,
                handshake_bytes: 1800,
                sni_offset: 1400,
                hybrid_key_bytes: 1216,
                authenticated_tls: true,
                application_ok: true,
            },
        ];
        assert!(round.complete());
        round.functions[0].tls_handshakes[1].application_ok = false;
        assert!(!round.complete());
        round.functions[0].tls_handshakes[1].application_ok = true;
        round.functions[0].tls_handshakes[1].authenticated_tls = false;
        assert!(!round.complete());
    }
    #[test]
    fn repeated_large_transfer_prefers_materially_faster_profile_but_keeps_uncertain_ties() {
        let make = |slow_ms, other_target: &str, bytes| {
            let mut t = Tournament::new(
                vec![candidate("slow", "window"), candidate("fast", "reorder")],
                3,
                6,
            );
            for (id, ms, target) in [
                ("slow", slow_ms, other_target),
                ("fast", 500, "https://cdn.test.invalid/same.js"),
            ]
            .into_iter()
            .cycle()
            .take(6)
            {
                let mut r = tests_round(id, true);
                let f = &mut r.functions[0];
                f.function = "client_assets".into();
                f.proof = "http_transfer";
                f.target = target.into();
                f.bytes = bytes + 70_000;
                f.elapsed_ms = ms + 300;
                f.transfer_bytes = bytes;
                f.transfer_ms = Some(ms);
                r.ordinal = t.rounds.len() + 1;
                r.runtime_generation = Some(r.ordinal as u64);
                t.record(r).unwrap();
            }
            t
        };
        let same = "https://cdn.test.invalid/same.js";
        let t = make(1000, same, super::super::zapret2_measure::TRANSFER_BYTES);
        assert_eq!(t.winner().as_deref(), Some("fast"));
        assert!(!t.tied());
        assert_eq!(t.finalists(1), ["fast"]);
        assert!(make(600, same, super::super::zapret2_measure::TRANSFER_BYTES).tied());
        assert!(make(
            1000,
            "https://cdn.test.invalid/other.js",
            super::super::zapret2_measure::TRANSFER_BYTES
        )
        .tied());
        assert!(make(1000, same, 1268).tied());
    }
    #[test]
    fn manual_application_reports_cannot_enter_the_automatic_tournament() {
        let mut tournament = Tournament::new(vec![candidate("a", "split")], 2, 4);
        let mut round = tests_round("a", true);
        round.tester_version = super::super::zapret2_scenario::VERSION;
        assert!(tournament.record(round.clone()).is_err());
        round.tester_version = super::super::zapret2_measure::TESTER_VERSION;
        round.functions[0].proof = "user_report";
        assert!(tournament.record(round).is_err());
        assert_eq!(tournament.winner(), None);
    }
    fn candidate(id: &str, family: &str) -> Candidate {
        Candidate {
            profile_id: id.into(),
            label: id.into(),
            family: family.into(),
            fingerprint: id.into(),
        }
    }
    fn record(t: &mut Tournament, id: &str, passed: bool) {
        let mut round = tests_round(id, passed);
        round.ordinal = t.rounds.len() + 1;
        round.runtime_generation = Some(round.ordinal as u64);
        t.record(round).unwrap();
    }
    #[test]
    fn once_lucky_cannot_defeat_a_repeated_success_and_failures_are_not_erased() {
        let mut t = Tournament::new(
            vec![candidate("lucky", "split"), candidate("stable", "fake")],
            3,
            8,
        );
        record(&mut t, "lucky", true);
        record(&mut t, "stable", true);
        record(&mut t, "lucky", false);
        record(&mut t, "stable", true);
        record(&mut t, "stable", true);
        assert_eq!(t.winner().as_deref(), Some("stable"));
        let lucky = t
            .summary()
            .into_iter()
            .find(|s| s.profile_id == "lucky")
            .unwrap();
        assert_eq!(lucky.functions[0].failed, 1);
        assert_eq!(lucky.status, "partial");
    }
    #[test]
    fn equal_repeated_results_remain_a_tie_and_round_order_rotates() {
        let mut t = Tournament::new(vec![candidate("a", "split"), candidate("b", "fake")], 2, 4);
        for id in ["a", "b", "b", "a"] {
            record(&mut t, id, true);
        }
        assert!(t.tied());
        assert!(t.winner().is_none());
        let ids = vec!["a".into(), "b".into()];
        assert_eq!(confirmation_order(&ids, 0), ["b", "a"]);
        assert_eq!(confirmation_order(&ids, 1), ["a", "b"]);
    }
    #[test]
    fn preserve_complementary_partial_results_without_combining_them_into_a_winner() {
        let mut t = Tournament::new(vec![candidate("a", "split"), candidate("b", "fake")], 2, 4);
        for id in ["a", "b"] {
            let mut round = tests_round(id, id == "a");
            let mut cdn = round.functions[0].clone();
            cdn.function = "cdn".into();
            cdn.outcome = if id == "b" {
                Outcome::Passed
            } else {
                Outcome::Failed
            };
            round.functions.push(cdn);
            round.ordinal = t.rounds.len() + 1;
            round.runtime_generation = Some(round.ordinal as u64);
            t.record(round).unwrap();
        }
        assert!(t.finalists(2).is_empty());
        assert!(t.winner().is_none());
        assert!(t
            .summary()
            .iter()
            .all(|s| s.status == "partial" && s.functions.iter().any(|f| f.passed > 0)));
    }
    #[test]
    fn source_names_do_not_create_diversity_and_wrong_receipts_are_rejected() {
        let mut copy = candidate("copy", "renamed");
        copy.fingerprint = "a".into();
        let ordered = diverse(
            vec![
                candidate("a", "reorder"),
                candidate("sibling", "reorder"),
                copy,
                candidate("b", "hostfake"),
            ],
            Some("a"),
            3,
        );
        assert_eq!(
            ordered
                .iter()
                .map(|c| c.profile_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "sibling"]
        );
        let mut t = Tournament::new(ordered, 2, 3);
        let mut wrong = tests_round("a", true);
        wrong.fingerprint = "other".into();
        assert!(t.record(wrong).is_err());
        record(&mut t, "a", true);
        assert!(t.record(tests_round("a", true)).is_err());
    }
    #[test]
    fn no_confirmation_or_unknown_network_cannot_be_a_recommendation() {
        let mut t = Tournament::new(vec![candidate("a", "split")], 2, 3);
        record(&mut t, "a", true);
        assert!(t.winner().is_none());
        let mut round = tests_round("a", true);
        round.ordinal = 2;
        round.runtime_generation = Some(2);
        round.comparable = false;
        t.record(round).unwrap();
        assert!(t.winner().is_none());
        assert_eq!(t.summary()[0].functions[0].inconclusive, 1);
    }

    #[test]
    fn failed_opening_group_uses_remaining_budget_without_waiving_confirmations() {
        let entries = || {
            ["a", "b", "c", "d"]
                .into_iter()
                .map(|id| candidate(id, id))
                .collect()
        };
        let mut tournament = Tournament::new(entries(), 3, 7);
        for id in ["a", "b", "c"] {
            record(&mut tournament, id, false);
        }
        assert_eq!(tournament.next_stage(2), NextStage::Screening("d".into()));
        record(&mut tournament, "d", true);
        assert_eq!(
            tournament.next_stage(2),
            NextStage::Confirmation(vec!["d".into()])
        );
        assert_eq!(tournament.winner(), None);
        record(&mut tournament, "d", true);
        assert_eq!(tournament.winner(), None);
        record(&mut tournament, "d", true);
        assert_eq!(tournament.winner(), Some("d".into()));

        let mut short = Tournament::new(entries(), 2, 4);
        for id in ["a", "b", "c"] {
            record(&mut short, id, false);
        }
        record(&mut short, "d", true);
        assert_eq!(short.next_stage(2), NextStage::Finished);
        assert_eq!(short.winner(), None);
    }
}
