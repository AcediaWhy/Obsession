//! Immutable fail-closed package built before any Legacy recovery mutation.
//!
//! This layer does not stop or start processes. It joins the exact observer
//! fence, Job-owned process identity, protected catalog and current machine
//! network into two already verified plans that a later staged executor may
//! consume without accepting paths or arguments from IPC.

#![cfg(windows)]

use obsession_runtime_protocol::{
    DpiCategory, DpiEngine, DpiRuntimeOptions, DpiRuntimeSnapshot, DpiSelection, DpiStartRequest,
};
use obsession_runtime_reliability::legacy_reliability::assessment::AssessmentClassification;
use obsession_runtime_reliability::legacy_reliability::contracts::{
    IntentEnvelope, ProcessOwner, SensorGeneration,
};
use obsession_runtime_reliability::legacy_reliability::environment_gate::LocalNetworkSnapshot;
use obsession_runtime_reliability::legacy_reliability::recovery::RecoveryConfig;

use crate::legacy_reliability::{
    LegacyObserverStartPlan, LegacyRecoveryCandidateContext, LegacyRecoveryInput,
    LegacyRecoveryScan,
};
use crate::protected_layout::{VerifiedDpiPlan, VerifiedRuntimeCatalog};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyRecoveryPreflightKey {
    generation: u64,
    input: LegacyRecoveryInput,
    previous_owner: ProcessOwner,
    candidate: RecoveryConfig,
    registry_content_hash: [u8; 32],
}

pub(crate) struct PreparedLegacyRecovery {
    key: LegacyRecoveryPreflightKey,
    previous_plan: VerifiedDpiPlan,
    candidate_plan: VerifiedDpiPlan,
    previous_selections: Vec<DpiSelection>,
    candidate_selections: Vec<DpiSelection>,
    exclusive_target_count: usize,
    observer_plan: LegacyObserverStartPlan,
}

impl PreparedLegacyRecovery {
    pub(crate) fn key(&self) -> &LegacyRecoveryPreflightKey {
        &self.key
    }

    pub(crate) fn previous_plan(&self) -> &VerifiedDpiPlan {
        &self.previous_plan
    }

    pub(crate) fn candidate_plan(&self) -> &VerifiedDpiPlan {
        &self.candidate_plan
    }

    pub(crate) fn previous_selections(&self) -> &[DpiSelection] {
        &self.previous_selections
    }

    pub(crate) fn candidate_selections(&self) -> &[DpiSelection] {
        &self.candidate_selections
    }

    pub(crate) fn exclusive_target_count(&self) -> usize {
        self.exclusive_target_count
    }

    pub(crate) fn observer_plan(&self) -> &LegacyObserverStartPlan {
        &self.observer_plan
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyRecoveryPreflightError {
    InvalidScan,
    NetworkChanged,
    RuntimeChanged,
    PreviousOwnerChanged,
    CandidateChanged,
    ProtectedCatalogRejected,
    ProtectedResourceChanged,
    NoExclusiveTargets,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_legacy_recovery(
    catalog: &VerifiedRuntimeCatalog,
    runtime: &DpiRuntimeSnapshot,
    scan: &LegacyRecoveryScan,
    previous_owner: &ProcessOwner,
    candidate: &RecoveryConfig,
    candidate_context: &LegacyRecoveryCandidateContext,
    current_network: &LocalNetworkSnapshot,
) -> Result<PreparedLegacyRecovery, LegacyRecoveryPreflightError> {
    let input = scan
        .input
        .as_ref()
        .ok_or(LegacyRecoveryPreflightError::InvalidScan)?;
    if scan.generation == 0
        || scan.revision == 0
        || scan.session_id != input.fence.session_id
        || input.observation.session_id != input.fence.session_id
        || input.observation.sensor_generation != input.fence.sensor_generation
        || input.observation.category != input.fence.category
        || input.observation.lane_generation != input.fence.lane_generation
        || !matches!(
            input.observation.classification,
            AssessmentClassification::DpiSuspected | AssessmentClassification::DpiBlocked
        )
    {
        return Err(LegacyRecoveryPreflightError::InvalidScan);
    }
    if !current_network.online
        || !current_network.interface_up
        || !current_network.default_route_available
        || !current_network.gateway_reachable
        || !current_network.network_fingerprint.is_stable()
        || current_network.network_fingerprint != input.fence.network_fingerprint
    {
        return Err(LegacyRecoveryPreflightError::NetworkChanged);
    }
    if runtime.engine != DpiEngine::Legacy
        || runtime.generation != scan.generation
        || runtime.selections.is_empty()
    {
        return Err(LegacyRecoveryPreflightError::RuntimeChanged);
    }
    if previous_owner.pid == 0
        || previous_owner.process_start_identity.get() == 0
        || !previous_owner.owns(input.previous.fingerprint(), input.fence.lane_generation)
    {
        return Err(LegacyRecoveryPreflightError::PreviousOwnerChanged);
    }
    if !input.candidates.iter().any(|known| known == candidate)
        || candidate_context.exclusive_target_count == 0
    {
        return Err(if candidate_context.exclusive_target_count == 0 {
            LegacyRecoveryPreflightError::NoExclusiveTargets
        } else {
            LegacyRecoveryPreflightError::CandidateChanged
        });
    }

    let category = recovery_category(&input.fence.category)
        .ok_or(LegacyRecoveryPreflightError::InvalidScan)?;
    let selection_index = runtime
        .selections
        .iter()
        .position(|selection| {
            selection.category == category && selection.strategy_id == input.previous.config_id()
        })
        .ok_or(LegacyRecoveryPreflightError::RuntimeChanged)?;
    let previous_selections = runtime.selections.clone();
    let mut candidate_selections = previous_selections.clone();
    candidate_selections[selection_index].strategy_id = candidate.config_id().to_owned();

    let previous_request = request(previous_selections.clone());
    let candidate_request = request(candidate_selections.clone());
    let previous_plan = catalog
        .resolve_dpi_plan(&previous_request)
        .map_err(|_| LegacyRecoveryPreflightError::ProtectedCatalogRejected)?;
    let candidate_plan = catalog
        .resolve_dpi_plan(&candidate_request)
        .map_err(|_| LegacyRecoveryPreflightError::ProtectedCatalogRejected)?;
    previous_plan
        .reverify()
        .and_then(|()| candidate_plan.reverify())
        .map_err(|_| LegacyRecoveryPreflightError::ProtectedResourceChanged)?;
    let active_categories = runtime
        .selections
        .iter()
        .map(|selection| recovery_category_name(selection.category).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    if active_categories != candidate_context.lane_generations.keys().cloned().collect()
        || candidate_context
            .lane_generations
            .get(&input.fence.category)
            != Some(&input.fence.lane_generation)
    {
        return Err(LegacyRecoveryPreflightError::InvalidScan);
    }
    let mut candidate_lane_generations = candidate_context.lane_generations.clone();
    candidate_lane_generations.insert(
        input.fence.category.clone(),
        next_generation(input.fence.lane_generation),
    );
    let observer_plan = LegacyObserverStartPlan::recovery(
        scan.generation,
        input.fence.session_id,
        SensorGeneration::new(next_nonzero(input.fence.sensor_generation.get())),
        input.fence.network_fingerprint.clone(),
        candidate_lane_generations,
        &candidate_selections,
    )
    .ok_or(LegacyRecoveryPreflightError::InvalidScan)?;

    Ok(PreparedLegacyRecovery {
        key: LegacyRecoveryPreflightKey {
            generation: scan.generation,
            input: input.clone(),
            previous_owner: previous_owner.clone(),
            candidate: candidate.clone(),
            registry_content_hash: candidate_context.registry_content_hash,
        },
        previous_plan,
        candidate_plan,
        previous_selections,
        candidate_selections,
        exclusive_target_count: candidate_context.exclusive_target_count,
        observer_plan,
    })
}

pub(crate) fn preflight_key(
    scan: &LegacyRecoveryScan,
    previous_owner: &ProcessOwner,
    candidate: &RecoveryConfig,
    candidate_context: &LegacyRecoveryCandidateContext,
) -> Option<LegacyRecoveryPreflightKey> {
    Some(LegacyRecoveryPreflightKey {
        generation: scan.generation,
        input: scan.input.clone()?,
        previous_owner: previous_owner.clone(),
        candidate: candidate.clone(),
        registry_content_hash: candidate_context.registry_content_hash,
    })
}

/// Matches the complete coordinator action against the immutable package
/// without exposing the package's private scan, registry hash or candidate
/// order to the effects layer.
pub(crate) fn prepared_matches_preflight_action(
    prepared: &PreparedLegacyRecovery,
    envelope: &IntentEnvelope,
    previous: &RecoveryConfig,
    previous_owner: &ProcessOwner,
    candidate: &RecoveryConfig,
) -> bool {
    let key = &prepared.key;
    IntentEnvelope::from_fence(envelope.attempt_id, &key.input.fence) == *envelope
        && &key.input.previous == previous
        && &key.previous_owner == previous_owner
        && &key.candidate == candidate
        && key.input.candidates.iter().any(|known| known == candidate)
}

fn request(selections: Vec<DpiSelection>) -> DpiStartRequest {
    DpiStartRequest {
        engine: DpiEngine::Legacy,
        selections,
        options: DpiRuntimeOptions {
            zapret2_level: 0,
            legacy_reliability: true,
            zapret2_overrides: Vec::new(),
        },
    }
}

fn recovery_category(category: &str) -> Option<DpiCategory> {
    match category {
        "discord" => Some(DpiCategory::Discord),
        "youtube_twitch" => Some(DpiCategory::YoutubeTwitch),
        "gaming" => Some(DpiCategory::Gaming),
        "atrisk" | "at_risk" => Some(DpiCategory::AtRisk),
        "universal" => Some(DpiCategory::Universal),
        _ => None,
    }
}

fn recovery_category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

fn next_generation(
    current: obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration,
) -> obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration {
    obsession_runtime_reliability::legacy_reliability::contracts::LaneGeneration::new(next_nonzero(
        current.get(),
    ))
}

const fn next_nonzero(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_reliability::dpi_engine::EngineKind;
    use obsession_runtime_reliability::eyes::Diagnosis;
    use obsession_runtime_reliability::legacy_reliability::assessment::{
        AssessmentConfidence, EvidenceSummary, LaneAssessment, LanePhase,
    };
    use obsession_runtime_reliability::legacy_reliability::contracts::{
        AttemptId, ConfigFingerprint, ConfirmationFailure, ExecutorOutcome, ExecutorStage,
        EyeHealthCounters, EyeHealthState, IntentEnvelope, IntentFence, LaneGeneration,
        NetworkFingerprint, ProcessStartIdentity, RegistryVersion, SensorGeneration, SessionId,
    };
    use obsession_runtime_reliability::legacy_reliability::manager::{
        AcceptedEventCounters, ConfirmationFlow, GapStatus, ObserveOnlyHealthStatus,
        ObserveOnlySessionStatus, ObserveOnlySnapshot, RejectedEventCounters,
    };
    use obsession_runtime_reliability::legacy_reliability::policy::PresumedIntent;
    use obsession_runtime_reliability::legacy_reliability::recovery::{
        RecoveryAction, RecoveryOrigin,
    };
    use obsession_runtime_reliability::legacy_reliability::recovery_runtime::IncidentObservation;
    use sha2::{Digest, Sha256};
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::dpi_executor::ProcessIdentity;
    use crate::legacy_confirmation::{LegacyHttpsProbeObservation, LegacyHttpsProbeResult};
    use crate::legacy_recovery_effects::{
        LegacyRecoveryNetworkPort, LegacyRecoveryObserverPort, LegacyRecoveryProbePort,
        LegacyRecoveryProcessView, LegacyRuntimeStartError, ProtectedLegacyRecoveryEffects,
    };
    use crate::legacy_recovery_transaction::{
        LegacyRecoveryEffectError, LegacyRecoveryEffects, LegacyRecoveryStageEffect,
    };
    use crate::legacy_reliability::LegacyObserverStartPlan;
    use crate::protected_layout::{ProtectedLayout, RESOURCE_MANIFEST};

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("obsession-preflight-test-{nonce}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn resource(root: &Path, relative: &str, bytes: &[u8]) -> serde_json::Value {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let sha256: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        serde_json::json!({
            "path": relative,
            "size": bytes.len(),
            "sha256": sha256,
        })
    }

    struct Fixture {
        _root: TestRoot,
        catalog: VerifiedRuntimeCatalog,
        candidate_path: PathBuf,
    }

    fn fixture() -> Fixture {
        let test = TestRoot::new();
        let program_files = test.0.join("Program Files");
        let install_root = program_files.join("Obsession");
        fs::create_dir_all(&program_files).unwrap();
        let executable = "runtime/legacy/winws.exe";
        let windivert = "runtime/legacy/WinDivert.dll";
        let previous = "runtime/configs/discord_1.conf";
        let candidate = "runtime/configs/discord_2.conf";
        let previous_list = "lists/discord-1.txt";
        let candidate_list = "lists/discord-2.txt";
        let files = vec![
            resource(&install_root, executable, b"protected-engine"),
            resource(&install_root, windivert, b"protected-windivert"),
            resource(
                &install_root,
                previous,
                b"--wf-tcp=443 --hostlist=lists\\discord-1.txt --new\n",
            ),
            resource(
                &install_root,
                candidate,
                b"--wf-tcp=443 --hostlist=lists\\discord-2.txt --new\n",
            ),
            resource(&install_root, previous_list, b"one.discord.example\n"),
            resource(&install_root, candidate_list, b"two.discord.example\n"),
        ];
        let manifest = install_root.join(RESOURCE_MANIFEST);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "engines": [{
                    "engine": "legacy",
                    "executable": executable,
                    "files": files,
                    "strategies": [
                        {
                            "id": "discord_1.conf",
                            "category": "discord",
                            "artifact": previous,
                            "dependencies": [windivert, previous_list]
                        },
                        {
                            "id": "discord_2.conf",
                            "category": "discord",
                            "artifact": candidate,
                            "dependencies": [windivert, candidate_list]
                        }
                    ]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let catalog = ProtectedLayout::inspect(&program_files, &install_root)
            .unwrap()
            .load_verified_catalog()
            .unwrap();
        Fixture {
            candidate_path: install_root.join(candidate),
            catalog,
            _root: test,
        }
    }

    fn recovery_state() -> (
        DpiRuntimeSnapshot,
        LegacyRecoveryScan,
        ProcessOwner,
        RecoveryConfig,
        LegacyRecoveryCandidateContext,
        LocalNetworkSnapshot,
    ) {
        let network_fingerprint = NetworkFingerprint::Stable {
            key: "stable-network".into(),
        };
        let previous = RecoveryConfig::new("discord_1.conf", "previous-fingerprint");
        let candidate = RecoveryConfig::new("discord_2.conf", "candidate-fingerprint");
        let fence = IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(7),
            sensor_generation: SensorGeneration::new(7),
            registry_version: RegistryVersion::new(11),
            network_fingerprint: network_fingerprint.clone(),
        };
        let scan = LegacyRecoveryScan {
            generation: 7,
            revision: 13,
            session_id: fence.session_id,
            input: Some(LegacyRecoveryInput {
                observation: IncidentObservation {
                    session_id: fence.session_id,
                    sensor_generation: fence.sensor_generation,
                    category: fence.category.clone(),
                    lane_generation: fence.lane_generation,
                    evidence_epoch: 17,
                    classification: AssessmentClassification::DpiSuspected,
                },
                fence: fence.clone(),
                previous: previous.clone(),
                candidates: vec![candidate.clone()],
            }),
        };
        let owner = ProcessOwner {
            pid: 19,
            process_start_identity: ProcessStartIdentity::new(23),
            config_fingerprint: ConfigFingerprint::new("previous-fingerprint"),
            lane_generation: fence.lane_generation,
        };
        (
            DpiRuntimeSnapshot {
                generation: 7,
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
            },
            scan,
            owner,
            candidate,
            LegacyRecoveryCandidateContext {
                registry_content_hash: [29; 32],
                exclusive_target_count: 2,
                lane_generations: std::collections::BTreeMap::from([(
                    "discord".into(),
                    LaneGeneration::new(7),
                )]),
            },
            LocalNetworkSnapshot {
                online: true,
                interface_up: true,
                default_route_available: true,
                gateway_reachable: true,
                network_fingerprint,
            },
        )
    }

    struct FakeObserver {
        generation: Option<u64>,
        fence: Option<IntentFence>,
        fail_candidate_start: bool,
        confirmation_snapshots: VecDeque<ObserveOnlySnapshot>,
        confirmation_targets: Vec<String>,
        operations: Vec<&'static str>,
    }

    impl LegacyRecoveryObserverPort for FakeObserver {
        fn runtime_generation(&self) -> Option<u64> {
            self.generation
        }

        fn current_fence(&self, category: &str) -> Option<IntentFence> {
            self.fence
                .as_ref()
                .filter(|fence| fence.category == category)
                .cloned()
        }

        fn confirmation_snapshot(&mut self) -> Option<ObserveOnlySnapshot> {
            if self.confirmation_snapshots.len() > 1 {
                self.confirmation_snapshots.pop_front()
            } else {
                self.confirmation_snapshots.front().cloned()
            }
        }

        fn confirmation_targets(
            &self,
            category: &str,
            candidate: &RecoveryConfig,
        ) -> Option<Vec<String>> {
            (category == "discord"
                && candidate.config_id() == "discord_2.conf"
                && !self.confirmation_targets.is_empty())
            .then(|| self.confirmation_targets.clone())
        }

        fn stop(&mut self, generation: u64) -> Result<(), ()> {
            if self.generation != Some(generation) {
                return Err(());
            }
            self.operations.push("stop");
            self.generation = None;
            self.fence = None;
            Ok(())
        }

        fn start_recovery(
            &mut self,
            plan: LegacyObserverStartPlan,
            selections: &[DpiSelection],
        ) -> Result<(), ()> {
            let candidate = selections
                .first()
                .is_some_and(|selection| selection.strategy_id == "discord_2.conf");
            self.operations.push(if candidate {
                "start_candidate"
            } else {
                "start_previous"
            });
            if candidate && self.fail_candidate_start {
                return Err(());
            }
            self.generation = Some(plan.runtime_generation());
            self.fence = Some(IntentFence {
                session_id: plan.session_id(),
                category: "discord".into(),
                lane_generation: plan.lane_generation("discord").unwrap(),
                sensor_generation: plan.sensor_generation(),
                registry_version: RegistryVersion::new(if candidate { 12 } else { 11 }),
                network_fingerprint: NetworkFingerprint::Stable {
                    key: "stable-network".into(),
                },
            });
            Ok(())
        }

        fn rebind_generation(&mut self, expected: u64, replacement: u64) -> Result<(), ()> {
            if self.generation != Some(expected) || replacement == 0 {
                return Err(());
            }
            self.operations.push("rebind");
            self.generation = Some(replacement);
            Ok(())
        }
    }

    struct FakeExecutorView {
        runtime: Option<DpiRuntimeSnapshot>,
        processes: Vec<ProcessIdentity>,
        next_generation: u64,
        fail_stop: bool,
        candidate_start_error: Option<LegacyRuntimeStartError>,
        operations: Vec<&'static str>,
    }

    impl LegacyRecoveryProcessView for FakeExecutorView {
        fn runtime_snapshot(&self) -> Option<DpiRuntimeSnapshot> {
            self.runtime.clone()
        }

        fn process_identities(&self) -> Vec<ProcessIdentity> {
            self.processes.clone()
        }

        fn stop_runtime(&mut self, generation: u64) -> Result<(), ()> {
            if self.runtime.as_ref().map(|runtime| runtime.generation) != Some(generation) {
                return Err(());
            }
            self.operations.push("stop");
            if self.fail_stop {
                return Err(());
            }
            self.runtime = None;
            self.processes.clear();
            Ok(())
        }

        fn start_runtime(
            &mut self,
            plan: &VerifiedDpiPlan,
        ) -> Result<u64, LegacyRuntimeStartError> {
            let is_candidate = plan
                .strategies()
                .first()
                .is_some_and(|strategy| strategy.strategy_id() == "discord_2.conf");
            if is_candidate {
                if let Some(error) = self.candidate_start_error {
                    return Err(error);
                }
            }
            if self.runtime.is_some() || self.next_generation == 0 {
                return Err(LegacyRuntimeStartError::RuntimeFailure);
            }
            let generation = self.next_generation;
            self.next_generation += 1;
            self.operations.push("start");
            self.runtime = Some(DpiRuntimeSnapshot {
                generation,
                engine: plan.engine(),
                selections: plan
                    .strategies()
                    .iter()
                    .map(|strategy| DpiSelection {
                        category: strategy.category(),
                        strategy_id: strategy.strategy_id().to_owned(),
                    })
                    .collect(),
            });
            self.processes = vec![ProcessIdentity {
                pid: generation as u32 + 20,
                creation_time_100ns: generation + 24,
                executable_sha256: "22".repeat(32),
            }];
            Ok(generation)
        }
    }

    struct FakeNetwork(VecDeque<LocalNetworkSnapshot>);

    impl LegacyRecoveryNetworkPort for FakeNetwork {
        fn snapshot(&mut self) -> LocalNetworkSnapshot {
            if self.0.len() > 1 {
                self.0.pop_front().unwrap()
            } else {
                self.0.front().cloned().unwrap()
            }
        }
    }

    #[derive(Default)]
    struct FakeProbes {
        observations: VecDeque<Vec<LegacyHttpsProbeObservation>>,
        armed_targets: Vec<String>,
        deadline_at_ms: Option<u64>,
        cancellations: u64,
    }

    impl LegacyRecoveryProbePort for FakeProbes {
        fn arm(&mut self, targets: &[String], deadline_at_monotonic_ms: u64) {
            self.armed_targets = targets.to_vec();
            self.deadline_at_ms = Some(deadline_at_monotonic_ms);
        }

        fn take_observations(&mut self, _now_ms: u64) -> Vec<LegacyHttpsProbeObservation> {
            self.observations.pop_front().unwrap_or_default()
        }

        fn cancel(&mut self) {
            self.cancellations = self.cancellations.saturating_add(1);
        }
    }

    fn fake_confirmation_snapshot(
        fence: &IntentFence,
        logical_now_ms: u64,
        flows: Vec<ConfirmationFlow>,
    ) -> ObserveOnlySnapshot {
        ObserveOnlySnapshot {
            session: ObserveOnlySessionStatus {
                session_id: fence.session_id,
                engine: EngineKind::Legacy,
                active_categories: vec![fence.category.clone()],
                network_fingerprint_at_start: fence.network_fingerprint.clone(),
                sensor_generation: fence.sensor_generation,
                target_registry_version: fence.registry_version,
                lane_generations: std::collections::BTreeMap::from([(
                    fence.category.clone(),
                    fence.lane_generation,
                )]),
                closed: false,
            },
            accepted: AcceptedEventCounters::default(),
            rejected: RejectedEventCounters::default(),
            gaps: GapStatus::default(),
            health: ObserveOnlyHealthStatus {
                state: EyeHealthState::Ready,
                counters: EyeHealthCounters::default(),
                last_reported_state: Some(EyeHealthState::Ready),
                last_reported_counters: Some(EyeHealthCounters::default()),
                receiver_failed: false,
            },
            logical_now_ms,
            last_gap_sequence: None,
            last_accepted_flow_sequence: flows.iter().map(|flow| flow.sequence).max(),
            pending_ingress_control_events: 0,
            pending_ingress_flow_events: std::collections::BTreeMap::new(),
            last_evicted_confirmation_flow_sequences: std::collections::BTreeMap::new(),
            lanes: vec![LaneAssessment {
                category: fence.category.clone(),
                lane_generation: fence.lane_generation,
                phase: LanePhase::Observing,
                classification: AssessmentClassification::AwaitingEvidence,
                confidence: AssessmentConfidence::None,
                evidence: EvidenceSummary::default(),
                working_confirmed_recently: false,
                evidence_epoch: 37,
                assessed_at_ms: logical_now_ms,
                cooldown_until_ms: None,
            }],
            presumed_intent: PresumedIntent::default(),
            active_configs: std::collections::BTreeMap::new(),
            candidate_configs: std::collections::BTreeMap::new(),
            gate_probe_hosts: std::collections::BTreeMap::new(),
            confirmation_flows: flows,
        }
    }

    fn confirmation_flow(
        sequence: u64,
        flow_id: u64,
        target: &str,
        sensor_ms: u64,
    ) -> ConfirmationFlow {
        ConfirmationFlow {
            sequence,
            category: "discord".into(),
            lane_generation: LaneGeneration::new(8),
            flow_id,
            target: target.into(),
            diagnosis: Diagnosis::Working,
            armed_at_sensor_ms: Some(sensor_ms.saturating_sub(1)),
            armed_at_capture_timestamp: Some(sensor_ms as i64),
            monotonic_ts: sensor_ms,
        }
    }

    fn successful_probe(
        probe_id: u64,
        target: &str,
        started_at_ms: u64,
    ) -> LegacyHttpsProbeObservation {
        LegacyHttpsProbeObservation {
            probe_id,
            domain: target.into(),
            started_at_monotonic_ms: started_at_ms,
            finished_at_monotonic_ms: started_at_ms.saturating_add(10),
            result: LegacyHttpsProbeResult::HttpResponse { status: 204 },
        }
    }

    fn preflight_action(scan: &LegacyRecoveryScan, owner: &ProcessOwner) -> RecoveryAction {
        let input = scan.input.as_ref().unwrap();
        RecoveryAction::Preflight {
            envelope: IntentEnvelope::from_fence(AttemptId::new(41), &input.fence),
            previous: input.previous.clone(),
            previous_owner: owner.clone(),
            candidate: input.candidates[0].clone(),
            origin: RecoveryOrigin::Assisted,
        }
    }

    fn fake_processes(owner: &ProcessOwner) -> Vec<ProcessIdentity> {
        vec![ProcessIdentity {
            pid: owner.pid,
            creation_time_100ns: owner.process_start_identity.get(),
            executable_sha256: "11".repeat(32),
        }]
    }

    #[test]
    fn preflight_preserves_neighbors_and_resolves_only_protected_plans() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, network) = recovery_state();
        let prepared = prepare_legacy_recovery(
            &fixture.catalog,
            &runtime,
            &scan,
            &owner,
            &candidate,
            &context,
            &network,
        )
        .unwrap();

        assert_eq!(prepared.previous_selections(), runtime.selections);
        assert_eq!(prepared.candidate_selections().len(), 1);
        assert_eq!(
            prepared.candidate_selections()[0].strategy_id,
            "discord_2.conf"
        );
        assert_eq!(prepared.previous_plan().strategies().len(), 1);
        assert_eq!(prepared.candidate_plan().strategies().len(), 1);
        assert_eq!(prepared.exclusive_target_count(), 2);
        assert_eq!(prepared.observer_plan().runtime_generation(), 7);
        assert_eq!(prepared.observer_plan().session_id(), SessionId::new(7));
        assert_eq!(
            prepared.observer_plan().sensor_generation(),
            SensorGeneration::new(8)
        );
        assert_eq!(
            prepared.observer_plan().lane_generation("discord"),
            Some(LaneGeneration::new(8))
        );
    }

    #[test]
    fn preflight_rejects_network_drift_and_post_catalog_tamper() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, mut network) = recovery_state();
        network.network_fingerprint = NetworkFingerprint::Stable {
            key: "different-network".into(),
        };
        assert!(matches!(
            prepare_legacy_recovery(
                &fixture.catalog,
                &runtime,
                &scan,
                &owner,
                &candidate,
                &context,
                &network,
            ),
            Err(LegacyRecoveryPreflightError::NetworkChanged)
        ));

        let (_, _, _, _, _, network) = recovery_state();
        fs::write(&fixture.candidate_path, b"tampered").unwrap();
        assert!(matches!(
            prepare_legacy_recovery(
                &fixture.catalog,
                &runtime,
                &scan,
                &owner,
                &candidate,
                &context,
                &network,
            ),
            Err(LegacyRecoveryPreflightError::ProtectedResourceChanged)
        ));
    }

    #[test]
    fn effects_preflight_rechecks_exact_state_and_arms_tentative_observer() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, network) = recovery_state();
        let prepared = prepare_legacy_recovery(
            &fixture.catalog,
            &runtime,
            &scan,
            &owner,
            &candidate,
            &context,
            &network,
        )
        .unwrap();
        let confirmation_fence = IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(8),
            sensor_generation: SensorGeneration::new(8),
            registry_version: RegistryVersion::new(12),
            network_fingerprint: NetworkFingerprint::Stable {
                key: "stable-network".into(),
            },
        };
        let mut observer = FakeObserver {
            generation: Some(runtime.generation),
            fence: Some(scan.input.as_ref().unwrap().fence.clone()),
            fail_candidate_start: false,
            confirmation_snapshots: VecDeque::from([
                fake_confirmation_snapshot(&confirmation_fence, 1_000, Vec::new()),
                fake_confirmation_snapshot(
                    &confirmation_fence,
                    1_021,
                    vec![
                        confirmation_flow(1, 101, "one.example", 1_020),
                        confirmation_flow(2, 102, "two.example", 1_021),
                    ],
                ),
            ]),
            confirmation_targets: vec!["one.example".into(), "two.example".into()],
            operations: Vec::new(),
        };
        let mut executor = FakeExecutorView {
            runtime: Some(runtime.clone()),
            processes: fake_processes(&owner),
            next_generation: 9,
            fail_stop: false,
            candidate_start_error: None,
            operations: Vec::new(),
        };
        let mut network_source = FakeNetwork(VecDeque::from([network.clone(), network.clone()]));
        let mut probes = FakeProbes {
            observations: VecDeque::from([vec![
                successful_probe(1, "one.example", 125),
                successful_probe(2, "two.example", 126),
            ]]),
            ..FakeProbes::default()
        };
        let mut effects = ProtectedLegacyRecoveryEffects::new(
            &prepared,
            &mut observer,
            &mut executor,
            &mut network_source,
            &mut probes,
        );

        let outcome = effects
            .execute(&preflight_action(&scan, &owner), 100)
            .unwrap();
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightPassed {
            refreshed_fence,
        }) = outcome
        else {
            panic!("exact preflight must arm the tentative observer");
        };
        assert_eq!(refreshed_fence.lane_generation, LaneGeneration::new(8));
        assert_eq!(refreshed_fence.sensor_generation, SensorGeneration::new(8));
        assert_eq!(refreshed_fence.registry_version, RegistryVersion::new(12));
        let current_envelope = IntentEnvelope::from_fence(AttemptId::new(41), &refreshed_fence);
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::StopPrevious {
                        envelope: current_envelope.clone(),
                        previous: scan.input.as_ref().unwrap().previous.clone(),
                        previous_owner: owner.clone(),
                        origin: RecoveryOrigin::Assisted,
                    },
                    110,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::Stopped {
                previous: owner.clone(),
            })
        );
        let start = effects
            .execute(
                &RecoveryAction::StartCandidate {
                    envelope: current_envelope.clone(),
                    candidate: candidate.clone(),
                    candidate_lane_generation: LaneGeneration::new(8),
                },
                120,
            )
            .unwrap();
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::Ready {
            candidate: candidate_owner,
        }) = start
        else {
            panic!("candidate generation must start with an exact owner");
        };
        assert!(candidate_owner.owns(candidate.fingerprint(), LaneGeneration::new(8)));
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::ConfirmCandidate {
                        envelope: current_envelope.clone(),
                        candidate: candidate.clone(),
                        owner: candidate_owner.clone(),
                    },
                    130,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Pending
        );
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::ConfirmCandidate {
                        envelope: current_envelope.clone(),
                        candidate: candidate.clone(),
                        owner: candidate_owner.clone(),
                    },
                    10_641,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::ConfirmationSucceeded {
                candidate: candidate_owner,
            })
        );
        let rollback = effects
            .execute(
                &RecoveryAction::RollbackPrevious {
                    envelope: current_envelope,
                    previous: scan.input.as_ref().unwrap().previous.clone(),
                    previous_lane_generation: LaneGeneration::new(9),
                    retry_pending: false,
                },
                10_650,
            )
            .unwrap();
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::RolledBack {
            previous: restored,
        }) = rollback
        else {
            panic!("failed candidate must restore the previous protected plan");
        };
        assert!(restored.owns(
            scan.input.as_ref().unwrap().previous.fingerprint(),
            LaneGeneration::new(9),
        ));
        drop(effects);
        assert_eq!(probes.armed_targets, ["one.example", "two.example"]);
        assert_eq!(probes.deadline_at_ms, Some(20_120));
        assert_eq!(probes.cancellations, 2);
        assert_eq!(
            observer.operations,
            [
                "stop",
                "start_candidate",
                "rebind",
                "stop",
                "start_previous",
                "rebind"
            ]
        );
        assert_eq!(executor.operations, ["stop", "start", "stop", "start"]);
        assert_eq!(executor.runtime.as_ref().unwrap().generation, 10);
        assert_eq!(
            observer.fence.as_ref().unwrap().lane_generation,
            LaneGeneration::new(9)
        );
    }

    #[test]
    fn emergency_transaction_failure_restores_previous_plan_and_releases_state() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, network) = recovery_state();
        let prepared = prepare_legacy_recovery(
            &fixture.catalog,
            &runtime,
            &scan,
            &owner,
            &candidate,
            &context,
            &network,
        )
        .unwrap();
        let mut observer = FakeObserver {
            generation: Some(runtime.generation),
            fence: Some(scan.input.as_ref().unwrap().fence.clone()),
            fail_candidate_start: false,
            confirmation_snapshots: VecDeque::new(),
            confirmation_targets: vec!["one.example".into(), "two.example".into()],
            operations: Vec::new(),
        };
        let mut executor = FakeExecutorView {
            runtime: Some(runtime),
            processes: fake_processes(&owner),
            next_generation: 9,
            fail_stop: false,
            candidate_start_error: None,
            operations: Vec::new(),
        };
        let mut network_source = FakeNetwork(VecDeque::from([network]));
        let mut probes = FakeProbes::default();
        let mut effects = ProtectedLegacyRecoveryEffects::new(
            &prepared,
            &mut observer,
            &mut executor,
            &mut network_source,
            &mut probes,
        );

        assert!(matches!(
            effects
                .execute(&preflight_action(&scan, &owner), 100)
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightPassed { .. })
        ));
        assert!(effects.emergency_rollback());
        let state = effects.into_state();
        assert!(state.is_idle());

        assert_eq!(probes.cancellations, 1);
        assert_eq!(executor.operations, ["stop", "start"]);
        assert_eq!(
            executor.runtime.as_ref().unwrap().selections,
            prepared.previous_selections()
        );
        assert_eq!(
            observer.operations,
            [
                "stop",
                "start_candidate",
                "stop",
                "start_previous",
                "rebind"
            ]
        );
        assert_eq!(
            observer.fence.as_ref().unwrap().lane_generation,
            LaneGeneration::new(9)
        );
    }

    #[test]
    fn exact_confirmation_commit_keeps_candidate_and_releases_rollback_state() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, network) = recovery_state();
        let prepared = prepare_legacy_recovery(
            &fixture.catalog,
            &runtime,
            &scan,
            &owner,
            &candidate,
            &context,
            &network,
        )
        .unwrap();
        let confirmation_fence = IntentFence {
            session_id: SessionId::new(7),
            category: "discord".into(),
            lane_generation: LaneGeneration::new(8),
            sensor_generation: SensorGeneration::new(8),
            registry_version: RegistryVersion::new(12),
            network_fingerprint: NetworkFingerprint::Stable {
                key: "stable-network".into(),
            },
        };
        let mut observer = FakeObserver {
            generation: Some(runtime.generation),
            fence: Some(scan.input.as_ref().unwrap().fence.clone()),
            fail_candidate_start: false,
            confirmation_snapshots: VecDeque::from([
                fake_confirmation_snapshot(&confirmation_fence, 1_000, Vec::new()),
                fake_confirmation_snapshot(
                    &confirmation_fence,
                    1_021,
                    vec![
                        confirmation_flow(1, 101, "one.example", 1_020),
                        confirmation_flow(2, 102, "two.example", 1_021),
                    ],
                ),
            ]),
            confirmation_targets: vec!["one.example".into(), "two.example".into()],
            operations: Vec::new(),
        };
        let mut executor = FakeExecutorView {
            runtime: Some(runtime),
            processes: fake_processes(&owner),
            next_generation: 9,
            fail_stop: false,
            candidate_start_error: None,
            operations: Vec::new(),
        };
        let mut network_source = FakeNetwork(VecDeque::from([network.clone()]));
        let mut probes = FakeProbes {
            observations: VecDeque::from([vec![
                successful_probe(1, "one.example", 125),
                successful_probe(2, "two.example", 126),
            ]]),
            ..FakeProbes::default()
        };
        let mut effects = ProtectedLegacyRecoveryEffects::new(
            &prepared,
            &mut observer,
            &mut executor,
            &mut network_source,
            &mut probes,
        );
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightPassed {
            refreshed_fence,
        }) = effects
            .execute(&preflight_action(&scan, &owner), 100)
            .unwrap()
        else {
            panic!("exact preflight must pass");
        };
        let envelope = IntentEnvelope::from_fence(AttemptId::new(41), &refreshed_fence);
        effects
            .execute(
                &RecoveryAction::StopPrevious {
                    envelope: envelope.clone(),
                    previous: scan.input.as_ref().unwrap().previous.clone(),
                    previous_owner: owner,
                    origin: RecoveryOrigin::Assisted,
                },
                110,
            )
            .unwrap();
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::Ready {
            candidate: candidate_owner,
        }) = effects
            .execute(
                &RecoveryAction::StartCandidate {
                    envelope: envelope.clone(),
                    candidate: candidate.clone(),
                    candidate_lane_generation: LaneGeneration::new(8),
                },
                120,
            )
            .unwrap()
        else {
            panic!("candidate must start");
        };
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::ConfirmCandidate {
                        envelope: envelope.clone(),
                        candidate: candidate.clone(),
                        owner: candidate_owner.clone(),
                    },
                    130,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Pending
        );
        assert!(matches!(
            effects
                .execute(
                    &RecoveryAction::ConfirmCandidate {
                        envelope: envelope.clone(),
                        candidate: candidate.clone(),
                        owner: candidate_owner.clone(),
                    },
                    10_641,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::ConfirmationSucceeded { .. })
        ));
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::CommitCandidate {
                        envelope: envelope.clone(),
                        candidate: candidate.clone(),
                        owner: candidate_owner.clone(),
                    },
                    10_642,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::CandidateCommitted {
                candidate: candidate_owner,
            })
        );
        assert_eq!(
            effects.execute(
                &RecoveryAction::RollbackPrevious {
                    envelope,
                    previous: scan.input.as_ref().unwrap().previous.clone(),
                    previous_lane_generation: LaneGeneration::new(9),
                    retry_pending: false,
                },
                10_650,
            ),
            Err(LegacyRecoveryEffectError::InvalidAction)
        );
        drop(effects);
        assert_eq!(executor.operations, ["stop", "start"]);
        assert_eq!(executor.runtime.as_ref().unwrap().generation, 9);
        assert_eq!(observer.generation, Some(9));
        assert_eq!(probes.cancellations, 3);
    }

    #[test]
    fn failed_stop_restores_previous_observer_when_previous_runtime_is_intact() {
        let fixture = fixture();
        let (runtime, scan, owner, candidate, context, network) = recovery_state();
        let prepared = prepare_legacy_recovery(
            &fixture.catalog,
            &runtime,
            &scan,
            &owner,
            &candidate,
            &context,
            &network,
        )
        .unwrap();
        let mut observer = FakeObserver {
            generation: Some(runtime.generation),
            fence: Some(scan.input.as_ref().unwrap().fence.clone()),
            fail_candidate_start: false,
            confirmation_snapshots: VecDeque::new(),
            confirmation_targets: Vec::new(),
            operations: Vec::new(),
        };
        let mut executor = FakeExecutorView {
            runtime: Some(runtime.clone()),
            processes: fake_processes(&owner),
            next_generation: 9,
            fail_stop: true,
            candidate_start_error: None,
            operations: Vec::new(),
        };
        let mut network_source = FakeNetwork(VecDeque::from([network]));
        let mut probes = FakeProbes::default();
        let mut effects = ProtectedLegacyRecoveryEffects::new(
            &prepared,
            &mut observer,
            &mut executor,
            &mut network_source,
            &mut probes,
        );
        let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightPassed {
            refreshed_fence,
        }) = effects
            .execute(&preflight_action(&scan, &owner), 100)
            .unwrap()
        else {
            panic!("exact preflight must pass");
        };
        let envelope = IntentEnvelope::from_fence(AttemptId::new(41), &refreshed_fence);
        assert_eq!(
            effects
                .execute(
                    &RecoveryAction::StopPrevious {
                        envelope,
                        previous: scan.input.as_ref().unwrap().previous.clone(),
                        previous_owner: owner,
                        origin: RecoveryOrigin::Assisted,
                    },
                    110,
                )
                .unwrap(),
            LegacyRecoveryStageEffect::Completed(ExecutorOutcome::ExecutionAborted {
                stage: ExecutorStage::Stop,
                reason: ConfirmationFailure::Sensor,
            })
        );
        drop(effects);
        assert_eq!(executor.runtime, Some(runtime));
        assert_eq!(executor.operations, ["stop"]);
        assert_eq!(
            observer.operations,
            ["stop", "start_candidate", "stop", "start_previous"]
        );
        assert_eq!(
            observer.fence.as_ref().unwrap().lane_generation,
            LaneGeneration::new(7)
        );
        assert_eq!(
            observer.fence.as_ref().unwrap().registry_version,
            RegistryVersion::new(11)
        );
    }

    #[test]
    fn candidate_start_failures_are_typed_and_leave_rollback_available() {
        for start_error in [
            LegacyRuntimeStartError::ProtectedResourceChanged,
            LegacyRuntimeStartError::ReadinessFailed,
            LegacyRuntimeStartError::RuntimeFailure,
        ] {
            let fixture = fixture();
            let (runtime, scan, owner, candidate, context, network) = recovery_state();
            let prepared = prepare_legacy_recovery(
                &fixture.catalog,
                &runtime,
                &scan,
                &owner,
                &candidate,
                &context,
                &network,
            )
            .unwrap();
            let mut observer = FakeObserver {
                generation: Some(runtime.generation),
                fence: Some(scan.input.as_ref().unwrap().fence.clone()),
                fail_candidate_start: false,
                confirmation_snapshots: VecDeque::new(),
                confirmation_targets: Vec::new(),
                operations: Vec::new(),
            };
            let mut executor = FakeExecutorView {
                runtime: Some(runtime),
                processes: fake_processes(&owner),
                next_generation: 9,
                fail_stop: false,
                candidate_start_error: Some(start_error),
                operations: Vec::new(),
            };
            let mut network_source = FakeNetwork(VecDeque::from([network]));
            let mut probes = FakeProbes::default();
            let mut effects = ProtectedLegacyRecoveryEffects::new(
                &prepared,
                &mut observer,
                &mut executor,
                &mut network_source,
                &mut probes,
            );

            let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightPassed {
                refreshed_fence,
            }) = effects
                .execute(&preflight_action(&scan, &owner), 100)
                .unwrap()
            else {
                panic!("exact preflight must pass before the start failure");
            };
            let envelope = IntentEnvelope::from_fence(AttemptId::new(41), &refreshed_fence);
            assert_eq!(
                effects
                    .execute(
                        &RecoveryAction::StopPrevious {
                            envelope: envelope.clone(),
                            previous: scan.input.as_ref().unwrap().previous.clone(),
                            previous_owner: owner.clone(),
                            origin: RecoveryOrigin::Assisted,
                        },
                        110,
                    )
                    .unwrap(),
                LegacyRecoveryStageEffect::Completed(ExecutorOutcome::Stopped { previous: owner })
            );
            let expected = match start_error {
                LegacyRuntimeStartError::ProtectedResourceChanged => {
                    ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Start,
                        reason: ConfirmationFailure::Sensor,
                    }
                }
                LegacyRuntimeStartError::ReadinessFailed => ExecutorOutcome::StartFailed {
                    candidate_fingerprint: candidate.fingerprint().clone(),
                },
                LegacyRuntimeStartError::RuntimeFailure => ExecutorOutcome::ExecutionAborted {
                    stage: ExecutorStage::Start,
                    reason: ConfirmationFailure::Environment,
                },
            };
            assert_eq!(
                effects
                    .execute(
                        &RecoveryAction::StartCandidate {
                            envelope: envelope.clone(),
                            candidate: candidate.clone(),
                            candidate_lane_generation: LaneGeneration::new(8),
                        },
                        120,
                    )
                    .unwrap(),
                LegacyRecoveryStageEffect::Completed(expected)
            );

            let rollback = effects
                .execute(
                    &RecoveryAction::RollbackPrevious {
                        envelope,
                        previous: scan.input.as_ref().unwrap().previous.clone(),
                        previous_lane_generation: LaneGeneration::new(9),
                        retry_pending: false,
                    },
                    130,
                )
                .unwrap();
            let LegacyRecoveryStageEffect::Completed(ExecutorOutcome::RolledBack {
                previous: restored,
            }) = rollback
            else {
                panic!("every typed start failure must preserve the rollback path");
            };
            assert!(restored.owns(
                scan.input.as_ref().unwrap().previous.fingerprint(),
                LaneGeneration::new(9),
            ));
        }
    }

    #[test]
    fn effects_preflight_restores_previous_observer_on_start_or_postcheck_failure() {
        for post_network_changed in [false, true] {
            let fixture = fixture();
            let (runtime, scan, owner, candidate, context, network) = recovery_state();
            let prepared = prepare_legacy_recovery(
                &fixture.catalog,
                &runtime,
                &scan,
                &owner,
                &candidate,
                &context,
                &network,
            )
            .unwrap();
            let mut observer = FakeObserver {
                generation: Some(runtime.generation),
                fence: Some(scan.input.as_ref().unwrap().fence.clone()),
                fail_candidate_start: !post_network_changed,
                confirmation_snapshots: VecDeque::new(),
                confirmation_targets: Vec::new(),
                operations: Vec::new(),
            };
            let mut executor = FakeExecutorView {
                runtime: Some(runtime.clone()),
                processes: fake_processes(&owner),
                next_generation: 9,
                fail_stop: false,
                candidate_start_error: None,
                operations: Vec::new(),
            };
            let mut changed = network.clone();
            changed.network_fingerprint = NetworkFingerprint::Stable {
                key: "changed-network".into(),
            };
            let mut network_source = FakeNetwork(VecDeque::from([
                network.clone(),
                if post_network_changed {
                    changed
                } else {
                    network.clone()
                },
            ]));
            let mut probes = FakeProbes::default();
            let mut effects = ProtectedLegacyRecoveryEffects::new(
                &prepared,
                &mut observer,
                &mut executor,
                &mut network_source,
                &mut probes,
            );

            let outcome = effects
                .execute(&preflight_action(&scan, &owner), 100)
                .unwrap();
            assert!(matches!(
                outcome,
                LegacyRecoveryStageEffect::Completed(ExecutorOutcome::PreflightRejected)
                    | LegacyRecoveryStageEffect::Completed(ExecutorOutcome::ExecutionAborted {
                        stage: ExecutorStage::Preflight,
                        ..
                    })
            ));
            drop(effects);
            assert_eq!(
                observer.fence.as_ref().unwrap().lane_generation,
                LaneGeneration::new(7)
            );
            assert_eq!(
                observer.fence.as_ref().unwrap().registry_version,
                RegistryVersion::new(11)
            );
            assert_eq!(executor.runtime, Some(runtime));
            assert_eq!(
                observer.operations,
                if post_network_changed {
                    vec!["stop", "start_candidate", "stop", "start_previous"]
                } else {
                    vec!["stop", "start_candidate", "start_previous"]
                }
            );
        }
    }
}
