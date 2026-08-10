//! Pure compatibility adapter from the existing TCP Eyes observation to the
//! generation-fenced Legacy reliability contract.

use std::collections::BTreeMap;

use crate::eyes::{Diagnosis, Observation};

use super::contracts::{EventEnvelope, FlowEvent, FlowEventError, LaneGeneration, Transport};
use super::target_registry::{Attribution, TargetOwner, TargetRegistry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowAttribution {
    Matched {
        target: String,
        owner: TargetOwner,
    },
    Ambiguous {
        target: String,
        owners: Vec<TargetOwner>,
    },
    Excluded {
        target: String,
        owners: Vec<TargetOwner>,
    },
    Unmatched,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptedFlow {
    pub event: FlowEvent,
    pub attribution: FlowAttribution,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdaptError {
    RegistryVersionMismatch,
    MissingLane { category: String },
    InvalidFlow(FlowEventError),
}

/// Converts the compatibility Observation without performing I/O or mutating
/// manager state. Ambiguous, excluded and unmatched flows remain diagnostics
/// but carry no category/lane and therefore cannot influence policy.
pub fn adapt_observation(
    observation: Observation,
    envelope: EventEnvelope,
    registry: &TargetRegistry,
    lane_generations: &BTreeMap<String, LaneGeneration>,
) -> Result<AdaptedFlow, AdaptError> {
    if envelope.target_registry_version != registry.version() {
        return Err(AdaptError::RegistryVersionMismatch);
    }

    // IP-only SYN attribution stays diagnostic because shared CDN addresses
    // cannot prove ownership. `process_socket_syn_no_synack` is emitted only
    // after the protected service correlates the exact FlowKey with a Windows
    // TCP owner PID, so it may pass through normal registry attribution.
    let diagnostic_only = observation.evidence == "syn_no_synack";
    let (category, lane_generation, attribution) = if diagnostic_only {
        (None, None, FlowAttribution::Unmatched)
    } else {
        match registry.attribute_active(&observation.domain) {
            Attribution::Matched { target, owner } => {
                let lane_generation =
                    lane_generations
                        .get(&owner.category)
                        .copied()
                        .ok_or_else(|| AdaptError::MissingLane {
                            category: owner.category.clone(),
                        })?;
                (
                    Some(owner.category.clone()),
                    Some(lane_generation),
                    FlowAttribution::Matched { target, owner },
                )
            }
            Attribution::Ambiguous { target, owners } => {
                (None, None, FlowAttribution::Ambiguous { target, owners })
            }
            Attribution::Excluded { target, owners } => {
                (None, None, FlowAttribution::Excluded { target, owners })
            }
            Attribution::Unmatched => (None, None, FlowAttribution::Unmatched),
        }
    };

    let transport = if observation.remote_port == 80 {
        Transport::Tcp
    } else {
        Transport::Tls
    };
    let diagnosis = Diagnosis::from_verdict_evidence(observation.verdict, observation.evidence);
    let armed_at_sensor_ms = observation.armed_at_ms;
    let armed_at_capture_timestamp = observation.armed_at_capture_timestamp;
    let mut event = FlowEvent::new(
        envelope,
        category,
        lane_generation,
        observation.flow_id,
        observation.domain,
        observation.dst_ip,
        transport,
        diagnosis,
        observation.evidence,
        observation.ts_ms,
    )
    .map_err(AdaptError::InvalidFlow)?;
    if let Some(armed_at_sensor_ms) = armed_at_sensor_ms {
        event = event.with_armed_at_sensor_ms(armed_at_sensor_ms);
    }
    if let Some(capture_timestamp) = armed_at_capture_timestamp {
        event = event.with_armed_at_capture_timestamp(capture_timestamp);
    }

    Ok(AdaptedFlow { event, attribution })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};

    use crate::eyes::Verdict;
    use crate::legacy_reliability::contracts::{RegistryVersion, SensorGeneration, SessionId};
    use crate::legacy_reliability::target_registry::LegacyConfigRecord;

    use super::*;

    fn registry(records: Vec<LegacyConfigRecord>) -> TargetRegistry {
        let active = records
            .iter()
            .map(|record| (record.category.clone(), record.config_name.clone()))
            .collect::<Vec<_>>();
        TargetRegistry::from_records_with_active_selections(records, active).unwrap()
    }

    fn record(category: &str, list_name: &str, domains: &str) -> LegacyConfigRecord {
        LegacyConfigRecord::new(
            category,
            format!("{category}.conf"),
            format!("--wf-tcp=80,443 --hostlist=lists/{list_name}"),
        )
        .with_hostlist(format!("lists/{list_name}"), domains)
    }

    fn observation(
        domain: &str,
        port: u16,
        verdict: Verdict,
        evidence: &'static str,
    ) -> Observation {
        Observation {
            flow_id: 17,
            domain: domain.to_string(),
            dst_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
            local_port: 50_000,
            remote_port: port,
            verdict,
            evidence,
            armed_at_ms: Some(33),
            armed_at_capture_timestamp: Some(3_300),
            ts_ms: 55,
        }
    }

    fn envelope(registry: &TargetRegistry) -> EventEnvelope {
        EventEnvelope::new(
            SessionId::new(1),
            SensorGeneration::new(2),
            RegistryVersion::new(registry.version().get()),
        )
    }

    #[test]
    fn matched_flow_gets_category_lane_and_typed_diagnosis() {
        let registry = registry(vec![record("video", "video.txt", "youtube.com\n")]);
        let adapted = adapt_observation(
            observation("www.youtube.com", 443, Verdict::Reset, "inbound_rst"),
            envelope(&registry),
            &registry,
            &BTreeMap::from([("video".to_string(), LaneGeneration::new(9))]),
        )
        .unwrap();

        assert_eq!(adapted.event.category.as_deref(), Some("video"));
        assert_eq!(adapted.event.lane_generation, Some(LaneGeneration::new(9)));
        assert_eq!(adapted.event.diagnosis, Diagnosis::TcpReset);
        assert_eq!(adapted.event.transport, Transport::Tls);
        assert_eq!(adapted.event.flow_id, 17);
        assert_eq!(adapted.event.armed_at_sensor_ms, Some(33));
        assert_eq!(adapted.event.armed_at_capture_timestamp, Some(3_300));
        assert!(matches!(
            adapted.attribution,
            FlowAttribution::Matched { .. }
        ));
    }

    #[test]
    fn ambiguous_flow_is_diagnostic_only() {
        let registry = registry(vec![
            record("first", "first.txt", "shared.example\n"),
            record("second", "second.txt", "shared.example\n"),
        ]);
        let lanes = BTreeMap::from([
            ("first".to_string(), LaneGeneration::new(1)),
            ("second".to_string(), LaneGeneration::new(1)),
        ]);
        let adapted = adapt_observation(
            observation("cdn.shared.example", 443, Verdict::Working, "server_hello"),
            envelope(&registry),
            &registry,
            &lanes,
        )
        .unwrap();

        assert_eq!(adapted.event.category, None);
        assert_eq!(adapted.event.lane_generation, None);
        assert!(matches!(
            adapted.attribution,
            FlowAttribution::Ambiguous { .. }
        ));
    }

    #[test]
    fn inactive_deeper_candidate_cannot_steal_an_active_lane_flow() {
        let active_generic = LegacyConfigRecord::new(
            "alpha",
            "alpha.conf",
            "--wf-tcp=443 --hostlist=lists/alpha.txt",
        )
        .with_hostlist("lists/alpha.txt", "example.com\n");
        let active_other = LegacyConfigRecord::new(
            "beta",
            "beta_active.conf",
            "--wf-tcp=443 --hostlist=lists/beta-active.txt",
        )
        .with_hostlist("lists/beta-active.txt", "beta.example\n");
        let inactive_deeper = LegacyConfigRecord::new(
            "beta",
            "beta_candidate.conf",
            "--wf-tcp=443 --hostlist=lists/beta-candidate.txt",
        )
        .with_hostlist("lists/beta-candidate.txt", "api.example.com\n");
        let registry = TargetRegistry::from_records_with_active_selections(
            [active_generic, active_other, inactive_deeper],
            [("alpha", "alpha.conf"), ("beta", "beta_active.conf")],
        )
        .unwrap();

        let adapted = adapt_observation(
            observation("api.example.com", 443, Verdict::Reset, "inbound_rst"),
            envelope(&registry),
            &registry,
            &BTreeMap::from([
                ("alpha".to_string(), LaneGeneration::new(11)),
                ("beta".to_string(), LaneGeneration::new(22)),
            ]),
        )
        .unwrap();

        assert_eq!(adapted.event.category.as_deref(), Some("alpha"));
        assert_eq!(adapted.event.lane_generation, Some(LaneGeneration::new(11)));
        assert!(matches!(
            adapted.attribution,
            FlowAttribution::Matched { ref owner, .. } if owner.category == "alpha"
        ));
    }

    #[test]
    fn syn_blackhole_without_socket_correlation_is_diagnostic_only() {
        let registry = registry(vec![record("video", "video.txt", "youtube.com\n")]);
        let adapted = adapt_observation(
            observation("www.youtube.com", 443, Verdict::Blackhole, "syn_no_synack"),
            envelope(&registry),
            &registry,
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(adapted.event.category, None);
        assert_eq!(adapted.event.lane_generation, None);
        assert_eq!(adapted.event.diagnosis, Diagnosis::TcpBlackhole);
        assert_eq!(adapted.attribution, FlowAttribution::Unmatched);
    }

    #[test]
    fn exact_process_socket_blackhole_gets_the_active_category_and_lane() {
        let registry = registry(vec![record("discord", "discord.txt", "discord.com\n")]);
        let adapted = adapt_observation(
            observation(
                "gateway.discord.com",
                443,
                Verdict::Blackhole,
                "process_socket_syn_no_synack",
            ),
            envelope(&registry),
            &registry,
            &BTreeMap::from([("discord".to_string(), LaneGeneration::new(7))]),
        )
        .unwrap();

        assert_eq!(adapted.event.category.as_deref(), Some("discord"));
        assert_eq!(adapted.event.lane_generation, Some(LaneGeneration::new(7)));
        assert_eq!(adapted.event.diagnosis, Diagnosis::TcpBlackhole);
        assert!(matches!(
            adapted.attribution,
            FlowAttribution::Matched { ref owner, .. } if owner.category == "discord"
        ));
    }

    #[test]
    fn synthetic_tcp_port_80_flow_is_not_automatic_evidence() {
        let registry = registry(vec![record("web", "web.txt", "example.com\n")]);
        let adapted = adapt_observation(
            observation("example.com", 80, Verdict::Working, "tls_app_data"),
            envelope(&registry),
            &registry,
            &BTreeMap::from([("web".to_string(), LaneGeneration::new(3))]),
        )
        .unwrap();

        assert_eq!(adapted.event.transport, Transport::Tcp);
        assert!(!adapted.event.transport.is_automatic_evidence());
    }

    #[test]
    fn mismatched_registry_version_is_rejected() {
        let registry = registry(vec![record("web", "web.txt", "example.com\n")]);
        let wrong = EventEnvelope::new(
            SessionId::new(1),
            SensorGeneration::new(1),
            RegistryVersion::new(registry.version().get().wrapping_add(1)),
        );
        assert_eq!(
            adapt_observation(
                observation("example.com", 443, Verdict::Working, "server_hello"),
                wrong,
                &registry,
                &BTreeMap::from([("web".to_string(), LaneGeneration::new(1))]),
            ),
            Err(AdaptError::RegistryVersionMismatch)
        );
    }
}
