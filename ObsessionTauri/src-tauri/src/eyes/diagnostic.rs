//! Bounded metadata for diagnostic TCP sockets, independent of policy verdicts.
//! Copies stay in RAM only while reconstructing the first TLS records. Public
//! snapshots contain no payload, SNI, process identity or absolute sequence.
use super::flow::strict_tls_working_evidence;
use super::parse::{extract_sni, ParsedPacket};
use obsession_runtime_protocol::DpiTcpFlow;
use std::{collections::HashMap, net::SocketAddr};

const MAX_FLOWS: usize = 128;
const PREFIX_CAP: usize = 16 * 1024;
const FLOW_TTL_MS: u64 = 30_000;

#[derive(Default)]
struct Prefix {
    base: Option<u32>,
    bytes: Vec<u8>,
    present: Vec<bool>,
    contiguous: usize,
    conflict: bool,
}

impl Prefix {
    fn anchor(&mut self, base: u32) {
        self.base = Some(base);
    }
    fn insert(&mut self, seq: u32, data: &[u8]) {
        let Some(base) = self.base else {
            return;
        };
        let offset = seq.wrapping_sub(base) as usize;
        // No guesses about data outside the SYN-anchored prefix. In particular
        // badseq fakes cannot become the base of the genuine ClientHello.
        if offset >= PREFIX_CAP || data.is_empty() {
            return;
        }
        let count = data.len().min(PREFIX_CAP - offset);
        self.bytes.resize(self.bytes.len().max(offset + count), 0);
        self.present
            .resize(self.present.len().max(offset + count), false);
        for (i, byte) in data[..count].iter().enumerate() {
            let at = offset + i;
            if self.present[at] && self.bytes[at] != *byte {
                self.conflict = true;
            }
            self.bytes[at] = *byte;
            self.present[at] = true;
        }
        while self.contiguous < self.present.len() && self.present[self.contiguous] {
            self.contiguous += 1;
        }
    }
    fn data(&self) -> &[u8] {
        if self.conflict {
            &[]
        } else {
            &self.bytes[..self.contiguous]
        }
    }
}

struct Flow {
    syn_seq: u32,
    metadata: DpiTcpFlow,
    outbound: Prefix,
    inbound: Prefix,
    payload_sequences: Vec<u32>,
    target: bool,
}

#[derive(Default)]
pub struct PacketTracker {
    flows: HashMap<(SocketAddr, SocketAddr), Flow>,
    pub capacity_evictions: u64,
}

impl PacketTracker {
    pub fn observe(&mut self, packet: &ParsedPacket, now_ms: u64) {
        if packet.key.remote_port != 443 {
            return;
        }
        let local = SocketAddr::new(packet.local_ip, packet.key.local_port);
        let remote = SocketAddr::new(packet.key.remote_ip, packet.key.remote_port);
        let key = (local, remote);
        self.flows
            .retain(|_, flow| now_ms.saturating_sub(flow.metadata.last_seen_ms) <= FLOW_TTL_MS);
        if packet.outbound && packet.flags.syn && !packet.flags.ack {
            // SYN retransmission retains the existing observations. Reuse of a
            // tuple with a new ISN starts a different flow.
            if self
                .flows
                .get(&key)
                .is_none_or(|flow| flow.syn_seq != packet.seq)
            {
                if self.flows.len() >= MAX_FLOWS && !self.flows.contains_key(&key) {
                    if let Some(oldest) = self
                        .flows
                        .iter()
                        .min_by_key(|(_, flow)| flow.metadata.last_seen_ms)
                        .map(|(key, _)| *key)
                    {
                        self.flows.remove(&oldest);
                        self.capacity_evictions = self.capacity_evictions.saturating_add(1);
                    }
                }
                let mut outbound = Prefix::default();
                outbound.anchor(packet.seq.wrapping_add(1));
                self.flows.insert(
                    key,
                    Flow {
                        syn_seq: packet.seq,
                        outbound,
                        inbound: Prefix::default(),
                        payload_sequences: Vec::new(),
                        target: false,
                        metadata: DpiTcpFlow {
                            local_address: local,
                            remote_address: remote,
                            first_seen_ms: now_ms,
                            last_seen_ms: now_ms,
                            syn_ack_observed: false,
                            client_hello_bytes: None,
                            acked_client_bytes: None,
                            outbound_payload_packets: 0,
                            repeated_sequence_packets: 0,
                            inbound_payload_bytes: 0,
                            inbound_reset: false,
                            inbound_fin: false,
                            tls_server_hello_observed: false,
                            tls_alert_observed: false,
                            conflicting_overlap: false,
                        },
                    },
                );
            }
        }
        let Some(flow) = self.flows.get_mut(&key) else {
            return;
        };
        flow.metadata.last_seen_ms = now_ms;
        if packet.outbound {
            if packet.payload.is_empty() {
                return;
            }
            flow.metadata.outbound_payload_packets =
                flow.metadata.outbound_payload_packets.saturating_add(1);
            if flow.payload_sequences.contains(&packet.seq) {
                flow.metadata.repeated_sequence_packets =
                    flow.metadata.repeated_sequence_packets.saturating_add(1);
            } else if flow.payload_sequences.len() < 128 {
                flow.payload_sequences.push(packet.seq);
            }
            // Low-TTL injections should not overwrite the genuine prefix. This
            // does not purport to identify every fake; conflicts remain explicit.
            if packet.ttl != 0 && packet.ttl <= 16 {
                return;
            }
            flow.outbound.insert(packet.seq, &packet.payload);
            if let Some(host) = extract_sni(flow.outbound.data()) {
                flow.target = matches!(
                    host.as_str(),
                    "discord.com"
                        | "gateway.discord.gg"
                        | "updates.discord.com"
                        | "www.youtube.com"
                        | "youtube.com"
                        | "i.ytimg.com"
                        | "github.com"
                        | "api.github.com"
                        | "www.roblox.com"
                        | "www.epicgames.com"
                );
                let data = flow.outbound.data();
                flow.metadata.client_hello_bytes =
                    Some(5 + u16::from_be_bytes([data[3], data[4]]) as u32);
            }
        } else {
            if packet.flags.syn && packet.flags.ack {
                if !flow.metadata.syn_ack_observed {
                    flow.inbound.anchor(packet.seq.wrapping_add(1));
                }
                flow.metadata.syn_ack_observed = true;
            }
            if packet.flags.ack {
                let acked = packet.ack_number.wrapping_sub(flow.syn_seq.wrapping_add(1));
                if acked <= 128 * 1024 {
                    flow.metadata.acked_client_bytes =
                        Some(flow.metadata.acked_client_bytes.unwrap_or(0).max(acked));
                }
            }
            flow.metadata.inbound_reset |= packet.flags.rst;
            flow.metadata.inbound_fin |= packet.flags.fin;
            flow.metadata.inbound_payload_bytes = flow
                .metadata
                .inbound_payload_bytes
                .saturating_add(packet.payload.len() as u64);
            flow.inbound.insert(packet.seq, &packet.payload);
            let data = flow.inbound.data();
            flow.metadata.tls_server_hello_observed |=
                strict_tls_working_evidence(data) == Some("server_hello");
            flow.metadata.tls_alert_observed |= data.len() >= 7
                && data[0] == 0x15
                && data[1] == 3
                && u16::from_be_bytes([data[3], data[4]]) >= 2
                && data.len() >= 5 + u16::from_be_bytes([data[3], data[4]]) as usize;
        }
        flow.metadata.conflicting_overlap = flow.outbound.conflict || flow.inbound.conflict;
    }

    pub fn snapshot(&self) -> Vec<DpiTcpFlow> {
        // Unrelated sockets stay private. A snapshot is observational; callers
        // must additionally match the exact local AND remote probe addresses.
        let mut flows = self
            .flows
            .values()
            .filter(|flow| flow.target)
            .map(|flow| flow.metadata.clone())
            .collect::<Vec<_>>();
        flows.sort_by_key(|flow| std::cmp::Reverse(flow.last_seen_ms));
        flows.truncate(32);
        flows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eyes::parse::{FlowKey, TcpFlags};
    fn packet(outbound: bool, seq: u32, ack: u32, payload: &[u8]) -> ParsedPacket {
        ParsedPacket {
            outbound,
            seq,
            ack_number: ack,
            local_ip: "192.0.2.1".parse().unwrap(),
            ttl: 64,
            key: FlowKey {
                local_port: 50000,
                remote_port: 443,
                remote_ip: "192.0.2.2".parse().unwrap(),
            },
            flags: TcpFlags {
                ack: true,
                ..Default::default()
            },
            payload: payload.into(),
        }
    }
    fn hello() -> Vec<u8> {
        let host = b"discord.com";
        let mut body = vec![3, 3];
        body.extend([0; 32]);
        body.extend([0, 0, 2, 0x13, 1, 1, 0]);
        let mut sni = ((host.len() + 3) as u16).to_be_bytes().to_vec();
        sni.push(0);
        sni.extend((host.len() as u16).to_be_bytes());
        sni.extend(host);
        let mut ext = vec![0, 0];
        ext.extend((sni.len() as u16).to_be_bytes());
        ext.extend(sni);
        body.extend((ext.len() as u16).to_be_bytes());
        body.extend(ext);
        let mut hs = vec![1, 0, 0, body.len() as u8];
        hs.extend(body);
        let mut record = vec![0x16, 3, 1];
        record.extend((hs.len() as u16).to_be_bytes());
        record.extend(hs);
        record
    }
    fn started(seq: u32) -> PacketTracker {
        let mut tracker = PacketTracker::default();
        let mut syn = packet(true, seq, 0, &[]);
        syn.flags = TcpFlags {
            syn: true,
            ..Default::default()
        };
        tracker.observe(&syn, 0);
        let mut synack = packet(false, 800, seq.wrapping_add(1), &[]);
        synack.flags.syn = true;
        tracker.observe(&synack, 1);
        tracker
    }
    #[test]
    fn split_out_of_order_hello_ack_wrap_and_reset_are_metadata_only() {
        let seq = u32::MAX - 20;
        let bytes = hello();
        let mut tracker = started(seq);
        tracker.observe(&packet(true, seq.wrapping_add(11), 801, &bytes[10..]), 2);
        assert!(tracker.snapshot().is_empty());
        tracker.observe(&packet(true, seq.wrapping_add(1), 801, &bytes[..10]), 3);
        tracker.observe(
            &packet(false, 801, seq.wrapping_add(1 + bytes.len() as u32), &[]),
            4,
        );
        let mut rst = packet(false, 801, seq.wrapping_add(1), &[]);
        rst.flags.rst = true;
        tracker.observe(&rst, 5);
        let snap = tracker.snapshot();
        let flow = &snap[0];
        assert_eq!(flow.client_hello_bytes, Some(bytes.len() as u32));
        assert_eq!(flow.acked_client_bytes, Some(bytes.len() as u32));
        assert!(flow.inbound_reset);
        assert!(!flow.tls_server_hello_observed);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("discord.com"));
    }
    #[test]
    fn fake_bad_sequence_and_midflow_capture_do_not_manufacture_hello() {
        let mut tracker = started(100);
        tracker.observe(&packet(true, 101u32.wrapping_sub(10000), 801, &hello()), 2);
        assert!(tracker.snapshot().is_empty());
        let mut absent = PacketTracker::default();
        absent.observe(&packet(true, 101, 801, &hello()), 2);
        assert!(absent.snapshot().is_empty());
    }
    #[test]
    fn conflicting_overlap_and_arbitrary_inbound_bytes_are_not_server_hello() {
        let mut tracker = started(100);
        tracker.observe(&packet(true, 101, 801, &hello()), 2);
        tracker.observe(&packet(false, 801, 200, b"HTTP/1.1 200 OK"), 3);
        assert!(!tracker.snapshot()[0].tls_server_hello_observed);
        tracker.observe(&packet(true, 101, 801, b"conflicting prefix"), 4);
        assert!(tracker.snapshot()[0].conflicting_overlap);
    }
    #[test]
    fn socket_reuse_and_capacity_are_bounded() {
        let mut tracker = started(100);
        tracker.observe(&packet(true, 101, 801, &hello()), 2);
        let mut syn = packet(true, 200, 0, &[]);
        syn.flags = TcpFlags {
            syn: true,
            ..Default::default()
        };
        tracker.observe(&syn, 3);
        assert!(tracker.snapshot().is_empty());
        for port in 1..200 {
            syn.key.local_port = port;
            tracker.observe(&syn, 4);
        }
        assert!(tracker.flows.len() <= MAX_FLOWS);
        assert!(tracker.capacity_evictions > 0);
    }
}
