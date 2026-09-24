//! Проверка вердиктов на последовательностях пакетов и меток времени.
//! Конвейер `parse → flow` воспроизводится без WinDivert и доступа к сети.

use std::net::{IpAddr, Ipv4Addr};

use crate::eyes::flow::{Config, FlowTable};
use crate::eyes::parse::{FlowKey, ParsedPacket, TcpFlags};
use crate::eyes::signal::{Observation, Verdict};

/// Событие сценария: пакет в момент `t_ms`, либо тик времени.
enum Ev {
    Pkt(ParsedPacket, u64),
    Tick(u64),
}

/// Прогоняет сценарий и собирает все вердикты в порядке появления.
fn run(cfg: Config, events: Vec<Ev>) -> Vec<Observation> {
    let mut table = FlowTable::new(cfg);
    let mut out = Vec::new();
    for ev in events {
        match ev {
            Ev::Pkt(p, t) => {
                if let Some(o) = table.on_packet(&p, t) {
                    out.push(o);
                }
            }
            Ev::Tick(t) => {
                for o in table.on_tick(t) {
                    out.push(o);
                }
            }
        }
    }
    out
}

// --- конструкторы пакетов для сценариев ---

const SRV: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));

fn fk(port: u16) -> FlowKey {
    FlowKey {
        local_port: port,
        remote_ip: SRV,
        remote_port: 443,
    }
}

fn p_syn(port: u16, ttl: u8, seq: u32) -> ParsedPacket {
    ParsedPacket {
        outbound: true,
        key: fk(port),
        ttl,
        seq,
        flags: TcpFlags {
            syn: true,
            ..Default::default()
        },
        payload: vec![],
    }
}

fn p_synack(port: u16) -> ParsedPacket {
    ParsedPacket {
        outbound: false,
        key: fk(port),
        ttl: 54,
        seq: 9000,
        flags: TcpFlags {
            syn: true,
            ack: true,
            ..Default::default()
        },
        payload: vec![],
    }
}

fn p_out(port: u16, ttl: u8, seq: u32, payload: Vec<u8>) -> ParsedPacket {
    ParsedPacket {
        outbound: true,
        key: fk(port),
        ttl,
        seq,
        flags: TcpFlags {
            psh: true,
            ack: true,
            ..Default::default()
        },
        payload,
    }
}

fn p_in_rst(port: u16) -> ParsedPacket {
    ParsedPacket {
        outbound: false,
        key: fk(port),
        ttl: 54,
        seq: 9001,
        flags: TcpFlags {
            rst: true,
            ..Default::default()
        },
        payload: vec![],
    }
}

fn p_in(port: u16, payload: Vec<u8>) -> ParsedPacket {
    ParsedPacket {
        outbound: false,
        key: fk(port),
        ttl: 54,
        seq: 9001,
        flags: TcpFlags {
            psh: true,
            ack: true,
            ..Default::default()
        },
        payload,
    }
}

fn ch(sni: &str) -> Vec<u8> {
    let host = sni.as_bytes();
    let mut sni_ext = Vec::new();
    let name_len = host.len() as u16;
    let list_len = 3 + name_len;
    sni_ext.extend_from_slice(&list_len.to_be_bytes());
    sni_ext.push(0x00);
    sni_ext.extend_from_slice(&name_len.to_be_bytes());
    sni_ext.extend_from_slice(host);
    let mut exts = Vec::new();
    exts.extend_from_slice(&0x0000u16.to_be_bytes());
    exts.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
    exts.extend_from_slice(&sni_ext);
    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&[7u8; 32]);
    body.push(0x00);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&[0x13, 0x01]);
    body.push(0x01);
    body.push(0x00);
    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);
    let mut hs = Vec::new();
    hs.push(0x01);
    let l = body.len() as u32;
    hs.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
    hs.extend_from_slice(&body);
    let mut rec = Vec::new();
    rec.push(0x16);
    rec.extend_from_slice(&[0x03, 0x01]);
    rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    rec.extend_from_slice(&hs);
    rec
}

fn sh() -> Vec<u8> {
    vec![0x16, 0x03, 0x03, 0x00, 0x04, 0x02, 0x00, 0x00, 0x00]
}

fn cfg() -> Config {
    Config {
        hostlist: vec!["youtube.com".into(), "discord.com".into()],
        ..Config::default()
    }
}

#[test]
fn scenario_clean_working() {
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40001, 128, 1000), 0),
            Ev::Pkt(p_synack(40001), 12),
            Ev::Pkt(p_out(40001, 128, 1001, ch("www.youtube.com")), 20),
            Ev::Pkt(p_in(40001, sh()), 45),
        ],
    );
    assert_eq!(out.len(), 1);
    let observation = &out[0];
    assert_eq!(observation.domain, "www.youtube.com");
    assert_eq!(observation.verdict, Verdict::Working);
    assert_eq!(observation.evidence, "server_hello");
    assert_eq!(observation.dst_ip, SRV);
    assert_eq!(observation.local_port, 40001);
    assert_eq!(observation.remote_port, 443);
    assert_ne!(observation.flow_id, 0);
    assert_eq!(observation.ts_ms, 45);
}

#[test]
fn scenario_dpi_reset() {
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40002, 128, 1000), 0),
            Ev::Pkt(p_synack(40002), 12),
            Ev::Pkt(p_out(40002, 128, 1001, ch("discord.com")), 20),
            Ev::Pkt(p_in_rst(40002), 33),
        ],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].domain, "discord.com");
    assert_eq!(out[0].verdict, Verdict::Reset);
    assert_eq!(out[0].evidence, "inbound_rst");
    assert_eq!(out[0].local_port, 40002);
    assert_eq!(out[0].ts_ms, 33);
}

#[test]
fn scenario_armed_blackhole_silence() {
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40003, 128, 1000), 0),
            Ev::Pkt(p_synack(40003), 12),
            Ev::Pkt(p_out(40003, 128, 1001, ch("www.youtube.com")), 20),
            // два ретрансмита CH — ответа нет (min_ch_retx=2)
            Ev::Pkt(p_out(40003, 128, 1001, ch("www.youtube.com")), 1020),
            Ev::Pkt(p_out(40003, 128, 1001, ch("www.youtube.com")), 2020),
            Ev::Tick(9000),
        ],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].domain, "www.youtube.com");
    assert_eq!(out[0].verdict, Verdict::Blackhole);
    assert_eq!(out[0].evidence, "silence+retransmit");
    assert_eq!(out[0].local_port, 40003);
    assert_eq!(out[0].ts_ms, 9000);
}

#[test]
fn scenario_split_and_fake_mixed() {
    // Реалистичный десинк: winws шлёт fake CH (низкий TTL, плохой SNI),
    // а реальный CH приходит двумя кусками в обратном порядке.
    let real = ch("www.youtube.com");
    let (a, b) = real.split_at(real.len() / 2);
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40004, 128, 1000), 0),
            Ev::Pkt(p_synack(40004), 8),
            // fake-инъекция winws: низкий TTL, левый SNI — должна игнорироваться
            Ev::Pkt(p_out(40004, 3, 1001, ch("www.google.com")), 15),
            // реальный CH, disorder: вторая половина, затем первая
            Ev::Pkt(p_out(40004, 128, 1001 + a.len() as u32, b.to_vec()), 18),
            Ev::Pkt(p_out(40004, 128, 1001, a.to_vec()), 19),
            Ev::Pkt(p_in(40004, sh()), 40),
        ],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].domain, "www.youtube.com");
    assert_eq!(out[0].verdict, Verdict::Working);
    assert_eq!(out[0].evidence, "server_hello");
}

#[test]
fn scenario_untracked_host_is_silent() {
    // Домен вне хостлиста — ни одного вердикта.
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40005, 128, 1000), 0),
            Ev::Pkt(p_synack(40005), 12),
            Ev::Pkt(p_out(40005, 128, 1001, ch("example.org")), 20),
            Ev::Pkt(p_in_rst(40005), 30),
            Ev::Tick(8000),
        ],
    );
    assert!(out.is_empty());
}

#[test]
fn scenario_multiple_flows_independent() {
    // Два параллельных соединения к разным хостам дают два независимых вердикта.
    let out = run(
        cfg(),
        vec![
            Ev::Pkt(p_syn(40006, 128, 1000), 0),
            Ev::Pkt(p_syn(40007, 128, 2000), 1),
            Ev::Pkt(p_out(40006, 128, 1001, ch("www.youtube.com")), 20),
            Ev::Pkt(p_out(40007, 128, 2001, ch("discord.com")), 21),
            Ev::Pkt(p_in(40006, sh()), 40),
            Ev::Pkt(p_in_rst(40007), 41),
        ],
    );
    assert_eq!(out.len(), 2);
    assert!(out.iter().any(|observation| {
        observation.domain == "www.youtube.com"
            && observation.verdict == Verdict::Working
            && observation.evidence == "server_hello"
    }));
    assert!(out.iter().any(|observation| {
        observation.domain == "discord.com"
            && observation.verdict == Verdict::Reset
            && observation.evidence == "inbound_rst"
    }));
    assert_ne!(out[0].flow_id, out[1].flow_id);
}
