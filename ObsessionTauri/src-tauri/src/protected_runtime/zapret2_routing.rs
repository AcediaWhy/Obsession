//! Generation-fenced engine observations for the probe's exact TCP socket.
//! A selected block and completed Lua action attest execution, not delivery of
//! injected packets or that the bypass caused the application's success.
use super::zapret2_measure::RoundMeasurement;
use obsession_runtime_protocol::{
    zapret2_composition::{DiscordComposition, DiscordScope},
    DpiDiagnostics,
};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConnectionEvidence {
    pub purpose: String,
    pub host: String,
    pub local_address: SocketAddr,
    pub remote_address: SocketAddr,
    pub expected_profile_id: String,
    pub observed_profiles: Vec<String>,
    pub observed_actions: Vec<String>,
    pub observed_sends: Vec<String>,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpConnectionProgress>,
}

/// Progress of this socket only; page headers cannot describe a later asset.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HttpConnectionProgress {
    pub stage: &'static str,
    pub started_after_ms: u64,
    pub elapsed_ms: u64,
    pub tls_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub bytes: usize,
}
impl ConnectionEvidence {
    pub fn new(stream: &tokio::net::TcpStream, host: &str, purpose: &str) -> std::io::Result<Self> {
        Ok(Self {
            purpose: purpose.into(),
            host: host.into(),
            local_address: stream.local_addr()?,
            remote_address: stream.peer_addr()?,
            expected_profile_id: String::new(),
            observed_profiles: vec![],
            observed_actions: vec![],
            observed_sends: vec![],
            status: "unverified",
            http: None,
        })
    }
}

struct Receipt {
    local: SocketAddr,
    remote: SocketAddr,
    profile: String,
    event: String,
    action: String,
}
fn endpoint(hex: &str, port: &str) -> Option<SocketAddr> {
    if !matches!(hex.len(), 8 | 32) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect::<Option<_>>()?;
    let ip = match bytes.len() {
        4 => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(bytes.as_slice()).ok()?)),
        16 => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(bytes.as_slice()).ok()?)),
        _ => return None,
    };
    let port: u16 = port.parse().ok()?;
    (port != 0).then_some(SocketAddr::new(ip, port))
}
fn parse(line: &str) -> Option<Receipt> {
    let fields: Vec<_> = line.split('|').collect();
    if fields.len() != 10
        || fields[..3] != ["OBS_DIAG", "flow_route", "v1"]
        || fields[3].is_empty()
        || fields[3].len() > 96
        || !fields[3]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        || !matches!(
            fields[8],
            "selected" | "action_ok" | "action_error" | "send_error" | "send_ok"
        )
    {
        return None;
    }
    Some(Receipt {
        local: endpoint(fields[4], fields[5])?,
        remote: endpoint(fields[6], fields[7])?,
        profile: fields[3].into(),
        event: fields[8].into(),
        action: fields[9].into(),
    })
}

pub(super) fn annotate(round: &mut RoundMeasurement, diagnostics: Result<&DpiDiagnostics, &str>) {
    let pair = DiscordComposition::parse(&round.profile_id).ok().flatten();
    // Keep bounded receive-side facts before the generation is stopped. Match
    // BOTH endpoints; unrelated/background traffic must never explain a probe.
    round.packet_capture = diagnostics.as_ref().ok().and_then(|value| {
        if Some(value.generation) != round.runtime_generation {
            return None;
        }
        let mut capture = value.packet_capture.clone()?;
        if capture.generation != value.generation {
            return None;
        }
        capture.flows.retain(|flow| {
            round
                .functions
                .iter()
                .flat_map(|f| &f.connections)
                .any(|c| {
                    c.local_address == flow.local_address && c.remote_address == flow.remote_address
                })
        });
        Some(capture)
    });
    let (receipts, available) = match diagnostics {
        Ok(value)
            if Some(value.generation) == round.runtime_generation
                && value.processes.len() == 1
                && value.processes[0].trace_enabled
                && value.processes[0].exit_code.is_none()
                && !value.processes[0].stream_closed
                && !value.processes[0].truncated =>
        {
            let lines = value.processes[0]
                .lines
                .iter()
                .filter(|line| line.starts_with("OBS_DIAG|flow_route|"));
            let parsed: Option<Vec<_>> = lines.map(|line| parse(line)).collect();
            match parsed {
                Some(values) => (values, true),
                None => (vec![], false),
            }
        }
        _ => (vec![], false),
    };
    for function in &mut round.functions {
        for connection in &mut function.connections {
            connection.expected_profile_id = pair
                .as_ref()
                .map(|pair| {
                    if DiscordScope::matches_cdn(&connection.host) {
                        pair.cdn_profile_id.clone()
                    } else {
                        pair.api_profile_id.clone()
                    }
                })
                .unwrap_or_else(|| round.profile_id.clone());
            let observed: Vec<_> = receipts
                .iter()
                .filter(|r| {
                    r.local == connection.local_address && r.remote == connection.remote_address
                })
                .collect();
            connection.observed_profiles = observed.iter().map(|r| r.profile.clone()).collect();
            connection.observed_profiles.sort();
            connection.observed_profiles.dedup();
            connection.observed_actions = observed
                .iter()
                .filter(|r| r.event == "action_ok")
                .map(|r| r.action.clone())
                .collect();
            connection.observed_actions.sort();
            connection.observed_actions.dedup();
            connection.observed_sends = observed
                .iter()
                .filter(|r| r.event == "send_ok")
                .map(|r| r.action.clone())
                .collect();
            connection.observed_sends.sort();
            connection.observed_sends.dedup();
            connection.status = if !available {
                "diagnostics_unavailable"
            } else if observed.is_empty() {
                "missing"
            } else if observed
                .iter()
                .any(|r| r.profile != connection.expected_profile_id)
            {
                "wrong_profile"
            } else if observed
                .iter()
                .any(|r| matches!(r.event.as_str(), "action_error" | "send_error"))
            {
                "action_error"
            } else if observed.iter().any(|r| r.event == "selected")
                && !connection.observed_actions.is_empty()
                && connection
                    .observed_sends
                    .iter()
                    .any(|action| connection.observed_actions.contains(action))
            {
                "verified"
            } else {
                "missing_action"
            };
        }
    }
    round.execution = if round
        .functions
        .iter()
        .filter(|f| f.required)
        .all(|f| f.routing_verified())
    {
        "flow_routing_verified"
    } else {
        "unverified"
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_protocol::DpiProcessDiagnostics;
    fn round() -> RoundMeasurement {
        let mut r =
            super::super::zapret2_tournament::tests_round("dc1_api-parent--cdn-parent", true);
        r.functions[0].connections = vec![ConnectionEvidence {
            purpose: "document".into(),
            host: "discord.com".into(),
            local_address: "192.0.2.1:50001".parse().unwrap(),
            remote_address: "203.0.113.1:443".parse().unwrap(),
            expected_profile_id: String::new(),
            observed_profiles: vec![],
            observed_actions: vec![],
            observed_sends: vec![],
            status: "unverified",
            http: None,
        }];
        let mut cdn = r.functions[0].clone();
        cdn.function = "cdn".into();
        cdn.connections[0].host = "cdn.discordapp.com".into();
        cdn.connections[0].local_address.set_port(50002);
        r.functions.push(cdn);
        r
    }
    fn line(profile: &str, port: u16, event: &str) -> String {
        format!("OBS_DIAG|flow_route|v1|{profile}|c0000201|{port}|cb007101|443|{event}|multisplit")
    }
    fn trace(lines: Vec<String>) -> DpiDiagnostics {
        DpiDiagnostics {
            generation: 1,
            packet_capture: None,
            processes: vec![DpiProcessDiagnostics {
                pid: 42,
                exit_code: None,
                trace_enabled: true,
                stream_closed: false,
                observed_bytes: 1000,
                discarded_lines: 0,
                truncated: false,
                lines,
            }],
        }
    }
    fn good() -> DpiDiagnostics {
        trace(
            [("api-parent", 50001), ("cdn-parent", 50002)]
                .into_iter()
                .flat_map(|(id, port)| {
                    [
                        line(id, port, "selected"),
                        line(id, port, "send_ok"),
                        line(id, port, "action_ok"),
                    ]
                })
                .collect(),
        )
    }
    #[test]
    fn both_exact_sockets_need_their_own_parent_and_completed_action() {
        let mut r = round();
        annotate(&mut r, Ok(&good()));
        assert!(r.complete());
        assert_eq!(r.execution, "flow_routing_verified");
        for mutation in 0..9 {
            let mut d = good();
            match mutation {
                0 => d.generation = 2,
                1 => d.processes[0].truncated = true,
                2 => d.processes[0].lines[5] = line("api-parent", 50002, "action_ok"),
                3 => d.processes[0].lines[5] = line("cdn-parent", 50003, "action_ok"),
                4 => d.processes[0]
                    .lines
                    .push(line("cdn-parent", 50002, "send_error")),
                5 => d.processes[0]
                    .lines
                    .push(line("unmatched", 50002, "selected")),
                6 => d.processes[0].lines[5] = "OBS_DIAG|flow_route|v1|malformed".into(),
                7 => d.processes[0].exit_code = Some(0),
                _ => d.processes[0]
                    .lines
                    .retain(|line| !line.contains("|send_ok|")),
            }
            let mut r = round();
            annotate(&mut r, Ok(&d));
            assert!(!r.complete(), "mutation {mutation}");
        }
    }
    #[test]
    fn packet_facts_are_retained_only_for_owned_sockets_and_never_promote_a_failed_probe() {
        use obsession_runtime_protocol::{DpiPacketCapture, DpiPacketCaptureState, DpiTcpFlow};
        let flow = DpiTcpFlow {
            local_address: "192.0.2.1:50001".parse().unwrap(),
            remote_address: "203.0.113.1:443".parse().unwrap(),
            first_seen_ms: 1,
            last_seen_ms: 100,
            syn_ack_observed: true,
            client_hello_bytes: Some(512),
            acked_client_bytes: Some(512),
            outbound_payload_packets: 3,
            repeated_sequence_packets: 1,
            inbound_payload_bytes: 100,
            inbound_reset: false,
            inbound_fin: false,
            tls_server_hello_observed: true,
            tls_alert_observed: false,
            conflicting_overlap: false,
        };
        let mut foreign = flow.clone();
        foreign.local_address.set_port(50009);
        let mut d = good();
        d.packet_capture = Some(DpiPacketCapture {
            generation: 1,
            state: DpiPacketCaptureState::Active,
            startup_error: None,
            captured_packets: 50,
            queue_drops: 2,
            parse_errors: 0,
            sensor_failed: false,
            capacity_evictions: 0,
            flows: vec![flow.clone(), foreign],
        });
        let mut r = round();
        r.functions[0].outcome = super::super::zapret2_measure::Outcome::Inconclusive;
        annotate(&mut r, Ok(&d));
        assert_eq!(r.packet_capture.as_ref().unwrap().flows, [flow]);
        assert_eq!(r.packet_capture.as_ref().unwrap().queue_drops, 2);
        assert!(!r.complete()); // ServerHello/ACK never stand in for application success.
        d.packet_capture.as_mut().unwrap().generation = 2;
        annotate(&mut r, Ok(&d));
        assert!(r.packet_capture.is_none());
        annotate(&mut r, Err("unavailable"));
        assert!(r.packet_capture.is_none());
    }

    #[test]
    fn ipv6_and_missing_diagnostics_do_not_fall_back_to_remote_ip_matching() {
        assert_eq!(
            endpoint("20010db8000000000000000000000001", "443").unwrap(),
            "[2001:db8::1]:443".parse().unwrap()
        );
        for (ip, port) in [("abc", "443"), ("c0000201", "0"), ("c0000201", "65536")] {
            assert!(endpoint(ip, port).is_none());
        }
        let mut r = round();
        annotate(&mut r, Err("unavailable"));
        assert!(!r.complete());
        assert!(r
            .functions
            .iter()
            .all(|f| f.connections[0].status == "diagnostics_unavailable"));
    }
}
