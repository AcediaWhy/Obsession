//! Pure observe-only Brain for Legacy lane assessments.
//!
//! The returned value is a presumed intent for UI/logging. This module has no
//! executor reference and therefore cannot stop, start, select, or persist a
//! configuration.

use std::collections::BTreeMap;

use serde::Serialize;

use super::assessment::{AssessmentClassification, LaneAssessment};

pub const BLACKHOLE_COOLDOWN_MS: u64 = 300_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PresumedIntent {
    Wait {
        reason: AssessmentClassification,
    },
    SwitchLane {
        category: String,
        candidate_config: String,
        reason: AssessmentClassification,
    },
    FreezeLane {
        category: String,
        until_ms: u64,
        reason: AssessmentClassification,
    },
}

impl Default for PresumedIntent {
    fn default() -> Self {
        Self::Wait {
            reason: AssessmentClassification::AwaitingEvidence,
        }
    }
}

impl PresumedIntent {
    const fn priority(&self) -> u8 {
        match self {
            Self::FreezeLane { .. } => 120,
            Self::Wait {
                reason:
                    AssessmentClassification::SensorUnreliable
                    | AssessmentClassification::Offline
                    | AssessmentClassification::DnsFailure
                    | AssessmentClassification::UpstreamDegraded,
            } => 110,
            // A global environment failure from any lane must suppress a
            // proposed switch from every lane. Freeze remains safe because it
            // only preserves the current config during a cooldown.
            Self::SwitchLane { .. } => 100,
            Self::Wait {
                reason:
                    AssessmentClassification::TargetUnavailable | AssessmentClassification::ServiceSlow,
            } => 70,
            Self::Wait {
                reason:
                    AssessmentClassification::DpiSuspected | AssessmentClassification::DpiBlocked,
            } => 60,
            Self::Wait {
                reason: AssessmentClassification::AwaitingEvidence,
            } => 10,
            Self::Wait {
                reason: AssessmentClassification::Working,
            } => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ObserveOnlyBrain;

impl ObserveOnlyBrain {
    pub fn decide(
        assessment: &LaneAssessment,
        active_config: Option<&str>,
        candidates: &[String],
        now_ms: u64,
    ) -> PresumedIntent {
        match assessment.classification {
            AssessmentClassification::DpiBlocked => PresumedIntent::FreezeLane {
                category: assessment.category.clone(),
                until_ms: assessment
                    .cooldown_until_ms
                    .unwrap_or_else(|| now_ms.saturating_add(BLACKHOLE_COOLDOWN_MS)),
                reason: AssessmentClassification::DpiBlocked,
            },
            AssessmentClassification::DpiSuspected => candidates
                .iter()
                .find(|candidate| Some(candidate.as_str()) != active_config)
                .cloned()
                .map_or(
                    PresumedIntent::Wait {
                        reason: AssessmentClassification::DpiSuspected,
                    },
                    |candidate_config| PresumedIntent::SwitchLane {
                        category: assessment.category.clone(),
                        candidate_config,
                        reason: AssessmentClassification::DpiSuspected,
                    },
                ),
            reason => PresumedIntent::Wait { reason },
        }
    }

    /// Chooses one compact public intent while assessments remain per-lane.
    /// Actions outrank waits; ties use the already deterministic category
    /// ordering of `BTreeMap`.
    pub fn decide_all(
        assessments: &[LaneAssessment],
        configs: &BTreeMap<String, LaneConfigOptions>,
        now_ms: u64,
    ) -> PresumedIntent {
        assessments
            .iter()
            .map(|assessment| {
                let options = configs.get(&assessment.category);
                Self::decide(
                    assessment,
                    options.and_then(|options| options.active.as_deref()),
                    options.map_or(&[], |options| options.candidates.as_slice()),
                    now_ms,
                )
            })
            .max_by_key(PresumedIntent::priority)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaneConfigOptions {
    pub active: Option<String>,
    pub candidates: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_reliability::assessment::{AssessmentConfidence, EvidenceSummary, LanePhase};
    use crate::legacy_reliability::contracts::LaneGeneration;

    fn assessment(classification: AssessmentClassification) -> LaneAssessment {
        LaneAssessment {
            category: "video".into(),
            lane_generation: LaneGeneration::new(7),
            phase: LanePhase::Suspect,
            classification,
            confidence: AssessmentConfidence::High,
            evidence: EvidenceSummary::default(),
            working_confirmed_recently: false,
            evidence_epoch: 1,
            assessed_at_ms: 10,
            cooldown_until_ms: None,
        }
    }

    #[test]
    fn reset_dpi_proposes_first_deterministic_non_active_candidate() {
        let intent = ObserveOnlyBrain::decide(
            &assessment(AssessmentClassification::DpiSuspected),
            Some("video_1.conf"),
            &[
                "video_1.conf".into(),
                "video_2.conf".into(),
                "video_3.conf".into(),
            ],
            100,
        );
        assert_eq!(
            intent,
            PresumedIntent::SwitchLane {
                category: "video".into(),
                candidate_config: "video_2.conf".into(),
                reason: AssessmentClassification::DpiSuspected,
            }
        );
    }

    #[test]
    fn no_candidate_and_environment_failures_only_wait() {
        assert_eq!(
            ObserveOnlyBrain::decide(
                &assessment(AssessmentClassification::DpiSuspected),
                Some("video_1.conf"),
                &["video_1.conf".into()],
                0,
            ),
            PresumedIntent::Wait {
                reason: AssessmentClassification::DpiSuspected,
            }
        );
        assert_eq!(
            ObserveOnlyBrain::decide(&assessment(AssessmentClassification::Offline), None, &[], 0,),
            PresumedIntent::Wait {
                reason: AssessmentClassification::Offline,
            }
        );
    }

    #[test]
    fn blackhole_proposes_freeze_but_executes_nothing() {
        assert_eq!(
            ObserveOnlyBrain::decide(
                &assessment(AssessmentClassification::DpiBlocked),
                None,
                &[],
                50,
            ),
            PresumedIntent::FreezeLane {
                category: "video".into(),
                until_ms: 300_050,
                reason: AssessmentClassification::DpiBlocked,
            }
        );
    }

    #[test]
    fn global_wait_keeps_the_most_actionable_environment_reason() {
        let mut working = assessment(AssessmentClassification::Working);
        working.category = "a_working".into();
        let mut offline = assessment(AssessmentClassification::Offline);
        offline.category = "b_offline".into();
        assert_eq!(
            ObserveOnlyBrain::decide_all(&[working, offline], &BTreeMap::new(), 0),
            PresumedIntent::Wait {
                reason: AssessmentClassification::Offline,
            }
        );
    }

    #[test]
    fn global_environment_failure_suppresses_switch_from_another_lane() {
        let mut suspected = assessment(AssessmentClassification::DpiSuspected);
        suspected.category = "a_suspected".into();
        let mut configs = BTreeMap::new();
        configs.insert(
            suspected.category.clone(),
            LaneConfigOptions {
                active: Some("video_1.conf".into()),
                candidates: vec!["video_1.conf".into(), "video_2.conf".into()],
            },
        );

        for classification in [
            AssessmentClassification::Offline,
            AssessmentClassification::DnsFailure,
            AssessmentClassification::UpstreamDegraded,
            AssessmentClassification::SensorUnreliable,
        ] {
            let mut environment = assessment(classification);
            environment.category = "z_environment".into();
            assert_eq!(
                ObserveOnlyBrain::decide_all(&[suspected.clone(), environment], &configs, 100,),
                PresumedIntent::Wait {
                    reason: classification,
                }
            );
        }
    }

    #[test]
    fn session_working_confirmation_is_ux_only_and_never_proposes_action() {
        let mut confirmed = assessment(AssessmentClassification::AwaitingEvidence);
        confirmed.working_confirmed_recently = true;
        assert_eq!(
            ObserveOnlyBrain::decide(
                &confirmed,
                Some("video_1.conf"),
                &["video_1.conf".into(), "video_2.conf".into()],
                100,
            ),
            PresumedIntent::Wait {
                reason: AssessmentClassification::AwaitingEvidence,
            }
        );
    }
}
