//! Автомат потока: из последовательности `ParsedPacket` (плюс тики времени)
//! выносит per-flow вердикт `working / reset / blackhole`.
//!
//! Всё на логическом времени (`ts_ms` приходит снаружи) — никаких `Instant`,
//! чтобы `replay.rs` мог гонять фикстуры детерминированно, без WinDivert и ОС.
//!
//! Ключевые решения:
//! - Вердикт эмитится ТОЛЬКО для потоков, дошедших до `Armed` (известен SNI),
//!   кроме blackhole на уровне SYN — там домен берём из learned IP→domain.
//! - `Reset` (входящий RST) против `Blackhole` (тишина + ретрансмиты) — та самая
//!   дискриминация: RST = «стратегия не пробила», тишина = «дроп по направлению».
//! - Пересборка исходящего ClientHello по seq чинит split/disorder-стратегии.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use crate::eyes::fake_filter::{is_winws_fake, FakeContext};
use crate::eyes::parse::{extract_sni, classify_record, ParsedPacket, TlsHandshake, TlsRecord};
use crate::eyes::signal::{Observation, Verdict};

/// Пороги и таймауты автомата. Все времена — в мс логического времени.
#[derive(Clone, Debug)]
pub struct Config {
    /// Суффиксы доменов, которые нас интересуют (хостлист). Пустой = следить за всеми.
    pub hostlist: Vec<String>,
    /// Максимум одновременно отслеживаемых потоков (защита от утечки).
    pub max_flows: usize,
    /// Нет SYN-ACK дольше этого + ретрансмиты SYN → blackhole на уровне рукопожатия.
    pub syn_synack_timeout_ms: u64,
    /// Armed без входящего ответа дольше этого → blackhole (если были ретрансмиты CH).
    pub armed_silence_timeout_ms: u64,
    /// Сколько держать «завершённый» поток, чтобы гасить поздние дубли пакетов.
    pub done_linger_ms: u64,
    /// Мин. число ретрансмитов SYN, чтобы счесть молчание блэкхолом (а не отменой).
    pub min_syn_retx: u8,
    /// Мин. число ретрансмитов ClientHello для blackhole в Armed.
    pub min_ch_retx: u8,
    /// Ёмкость кэша IP→domain (для атрибуции SYN-level blackhole).
    pub ip_cache_cap: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hostlist: Vec::new(),
            max_flows: 4096,
            syn_synack_timeout_ms: 3000,
            armed_silence_timeout_ms: 5000,
            done_linger_ms: 2000,
            min_syn_retx: 2,
            min_ch_retx: 1,
            ip_cache_cap: 1024,
        }
    }
}

impl Config {
    /// Суффиксное сравнение: `domain == s` или `domain` заканчивается на `.s`.
    fn hostlist_matches(&self, domain: &str) -> bool {
        if self.hostlist.is_empty() {
            return true;
        }
        self.hostlist.iter().any(|s| {
            let s = s.trim_start_matches('.');
            domain == s || domain.ends_with(&format!(".{s}"))
        })
    }
}

/// Фаза жизни потока.
#[derive(Clone, Debug)]
enum Phase {
    /// TCP ещё не установлен: ждём SYN-ACK, копим исходящий ClientHello.
    Handshake,
    /// ClientHello ушёл, SNI распознан и матчит хостлист — ждём вердикт.
    Armed { domain: String, t0: u64 },
    /// Вердикт уже вынесен — держим недолго, чтобы гасить поздние дубли.
    Done,
}

/// Пересборка исходящего ClientHello по TCP-seq (устойчива к split/disorder).
#[derive(Default)]
struct Reasm {
    base: Option<u32>,
    segs: BTreeMap<u32, Vec<u8>>,
    total: usize,
}

impl Reasm {
    const CAP: usize = 16 * 1024; // ClientHello крупнее этого нам не нужен

    /// Вставляет сегмент. Возвращает false, если это дубликат (ретрансмит) или мусор.
    fn insert(&mut self, seq: u32, data: &[u8]) -> bool {
        if data.is_empty() || self.total >= Self::CAP {
            return false;
        }
        if self.segs.contains_key(&seq) {
            return false; // ретрансмит того же сегмента
        }
        match self.base {
            None => self.base = Some(seq),
            Some(b) if seq_lt(seq, b) => self.base = Some(seq),
            _ => {}
        }
        self.total += data.len();
        self.segs.insert(seq, data.to_vec());
        true
    }

    /// Собирает непрерывный префикс от base, склеивая сегменты (с обрезкой перекрытий).
    fn contiguous(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut want = match self.base {
            Some(b) => b,
            None => return out,
        };
        loop {
            let mut advanced = false;
            for (&s, d) in &self.segs {
                let end = s.wrapping_add(d.len() as u32);
                // сегмент покрывает позицию want?
                if seq_le(s, want) && seq_lt(want, end) {
                    let off = want.wrapping_sub(s) as usize;
                    out.extend_from_slice(&d[off..]);
                    want = end;
                    advanced = true;
                    break;
                }
            }
            if !advanced {
                break;
            }
        }
        out
    }
}

/// Один отслеживаемый поток.
struct Flow {
    key_ip: IpAddr,
    local_port: u16,
    phase: Phase,
    fake_ctx: FakeContext,
    reasm: Reasm,
    created_ms: u64,
    last_seen_ms: u64,
    syn_seen: bool,
    syn_ack_seen: bool,
    syn_retx: u8,
    ch_retx: u8,
    inbound_data_seen: bool,
    emitted: bool,
}

impl Flow {
    fn new(key_ip: IpAddr, local_port: u16, now: u64) -> Self {
        Self {
            key_ip,
            local_port,
            phase: Phase::Handshake,
            fake_ctx: FakeContext::default(),
            reasm: Reasm::default(),
            created_ms: now,
            last_seen_ms: now,
            syn_seen: false,
            syn_ack_seen: false,
            syn_retx: 0,
            ch_retx: 0,
            inbound_data_seen: false,
            emitted: false,
        }
    }
}

/// Таблица потоков — единоличный владелец состояния (живёт в одном потоке-трекере).
pub struct FlowTable {
    cfg: Config,
    flows: HashMap<crate::eyes::parse::FlowKey, Flow>,
    /// Learned IP→domain: наполняется при arming, нужен для атрибуции SYN-level blackhole.
    ip_domain: HashMap<IpAddr, String>,
}

impl FlowTable {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            flows: HashMap::new(),
            ip_domain: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.flows.len()
    }

    /// Обрабатывает один пакет. Возвращает вердикт, если он готов.
    pub fn on_packet(&mut self, pkt: &ParsedPacket, ts_ms: u64) -> Option<Observation> {
        // Завершённый поток: гасим дубли, продлеваем linger.
        if let Some(f) = self.flows.get_mut(&pkt.key) {
            if f.emitted {
                f.last_seen_ms = ts_ms;
                return None;
            }
        }

        // Отсев собственных fake-инъекций winws (только исходящие).
        if pkt.outbound {
            let ctx = self
                .flows
                .get(&pkt.key)
                .map(|f| f.fake_ctx)
                .unwrap_or_default();
            if is_winws_fake(pkt, &ctx) {
                return None;
            }
        }

        if pkt.outbound {
            self.on_outbound(pkt, ts_ms)
        } else {
            self.on_inbound(pkt, ts_ms)
        }
    }

    fn on_outbound(&mut self, pkt: &ParsedPacket, now: u64) -> Option<Observation> {
        let flags = pkt.flags;

        // Исходящий SYN (без ACK) — начало соединения / baseline TTL,seq.
        if flags.syn && !flags.ack {
            if self.flows.len() >= self.cfg.max_flows && !self.flows.contains_key(&pkt.key) {
                self.evict_one();
            }
            let f = self
                .flows
                .entry(pkt.key)
                .or_insert_with(|| Flow::new(pkt.key.remote_ip, pkt.key.local_port, now));
            if f.syn_seen {
                f.syn_retx = f.syn_retx.saturating_add(1); // ретрансмит SYN
            } else {
                f.syn_seen = true;
                f.fake_ctx.note_syn(pkt.ttl, pkt.seq);
            }
            f.last_seen_ms = now;
            return None;
        }

        // Исходящие данные — кандидат в ClientHello (или его фрагмент).
        if !pkt.payload.is_empty() {
            // Поток мог не иметь SYN (мы подключились в середине) — создаём лениво.
            if !self.flows.contains_key(&pkt.key) {
                if self.flows.len() >= self.cfg.max_flows {
                    self.evict_one();
                }
                self.flows.insert(
                    pkt.key,
                    Flow::new(pkt.key.remote_ip, pkt.key.local_port, now),
                );
            }
            let f = self.flows.get_mut(&pkt.key).unwrap();
            f.last_seen_ms = now;

            // Уже вооружён? Тогда повтор исходящих данных = ретрансмит CH (признак blackhole).
            if matches!(f.phase, Phase::Armed { .. }) {
                if !f.reasm.insert(pkt.seq, &pkt.payload) {
                    f.ch_retx = f.ch_retx.saturating_add(1);
                }
                return None;
            }

            // Ещё в Handshake: копим сегменты, пробуем распарсить SNI.
            let is_new = f.reasm.insert(pkt.seq, &pkt.payload);
            if !is_new {
                f.ch_retx = f.ch_retx.saturating_add(1);
            }
            let assembled = f.reasm.contiguous();
            if let Some(sni) = extract_sni(&assembled) {
                if self.cfg.hostlist_matches(&sni) {
                    // Вооружаемся: запоминаем домен и IP→domain для будущих blackhole.
                    self.remember_ip_domain(pkt.key.remote_ip, &sni);
                    let f = self.flows.get_mut(&pkt.key).unwrap();
                    f.phase = Phase::Armed {
                        domain: sni,
                        t0: now,
                    };
                } else {
                    // Домен не из хостлиста — поток нам неинтересен.
                    self.flows.remove(&pkt.key);
                }
            }
            return None;
        }

        // Прочее (чистый ACK/FIN): просто отмечаем активность.
        if let Some(f) = self.flows.get_mut(&pkt.key) {
            f.last_seen_ms = now;
        }
        None
    }

    fn on_inbound(&mut self, pkt: &ParsedPacket, now: u64) -> Option<Observation> {
        let f = self.flows.get_mut(&pkt.key)?;
        f.last_seen_ms = now;

        // Входящий SYN-ACK — TCP встал, это уже не SYN-level blackhole.
        if pkt.flags.syn && pkt.flags.ack {
            f.syn_ack_seen = true;
            return None;
        }

        // Входящий RST: DPI срезал попытку. Вердикт только если знаем домен (Armed).
        if pkt.flags.rst {
            if let Phase::Armed { domain, .. } = &f.phase {
                let obs = Observation {
                    domain: domain.clone(),
                    dst_ip: f.key_ip,
                    local_port: f.local_port,
                    verdict: Verdict::Reset,
                    evidence: "inbound_rst",
                    ts_ms: now,
                };
                f.emitted = true;
                f.phase = Phase::Done;
                return Some(obs);
            }
            // RST до arming — домен неизвестен, молча закрываем.
            self.flows.remove(&pkt.key);
            return None;
        }

        // Входящие данные с полезной нагрузкой — соединение живо.
        if !pkt.payload.is_empty() {
            f.inbound_data_seen = true;
            let rec = classify_record(&pkt.payload);
            let is_alive = matches!(
                rec,
                TlsRecord::Handshake(TlsHandshake::ServerHello) | TlsRecord::AppData
            ) || !matches!(rec, TlsRecord::Alert);
            if is_alive {
                if let Phase::Armed { domain, .. } = &f.phase {
                    let obs = Observation {
                        domain: domain.clone(),
                        dst_ip: f.key_ip,
                        local_port: f.local_port,
                        verdict: Verdict::Working,
                        evidence: "server_hello",
                        ts_ms: now,
                    };
                    f.emitted = true;
                    f.phase = Phase::Done;
                    return Some(obs);
                }
            }
        }
        None
    }

    /// Тик времени: детект таймаутов (blackhole) и эвикция старья.
    /// Возвращает все вердикты, «созревшие» к этому моменту.
    pub fn on_tick(&mut self, now: u64) -> Vec<Observation> {
        let mut out = Vec::new();
        let mut to_remove: Vec<crate::eyes::parse::FlowKey> = Vec::new();

        // Собираем ключи заранее, чтобы не держать заём во время мутации.
        let keys: Vec<_> = self.flows.keys().copied().collect();
        for key in keys {
            let Some(f) = self.flows.get(&key) else {
                continue;
            };

            // Завершённые — снять после linger.
            if f.emitted {
                if now.saturating_sub(f.last_seen_ms) >= self.cfg.done_linger_ms {
                    to_remove.push(key);
                }
                continue;
            }

            match &f.phase {
                Phase::Handshake => {
                    let age = now.saturating_sub(f.created_ms);
                    if age >= self.cfg.syn_synack_timeout_ms {
                        // SYN-level blackhole: нет SYN-ACK + ретрансмиты SYN.
                        if !f.syn_ack_seen && f.syn_retx >= self.cfg.min_syn_retx {
                            // Домен неизвестен (SNI не видели) — берём из learned IP→domain.
                            if let Some(domain) = self.ip_domain.get(&f.key_ip).cloned() {
                                out.push(Observation {
                                    domain,
                                    dst_ip: f.key_ip,
                                    local_port: f.local_port,
                                    verdict: Verdict::Blackhole,
                                    evidence: "syn_no_synack",
                                    ts_ms: now,
                                });
                            }
                        }
                        to_remove.push(key); // в любом случае снимаем зависший handshake
                    }
                }
                Phase::Armed { domain, t0 } => {
                    let silent = now.saturating_sub(*t0);
                    if !f.inbound_data_seen && silent >= self.cfg.armed_silence_timeout_ms {
                        if f.ch_retx >= self.cfg.min_ch_retx {
                            out.push(Observation {
                                domain: domain.clone(),
                                dst_ip: f.key_ip,
                                local_port: f.local_port,
                                verdict: Verdict::Blackhole,
                                evidence: "silence+retransmit",
                                ts_ms: now,
                            });
                        }
                        to_remove.push(key);
                    }
                }
                Phase::Done => {}
            }
        }

        for key in to_remove {
            // Помечаем эмитнутые blackhole как done, чтобы не всплывали повторно —
            // но проще просто удалить: вердикт уже в out.
            self.flows.remove(&key);
        }
        out
    }

    /// Запоминает соответствие IP→domain (с грубым ограничением ёмкости).
    fn remember_ip_domain(&mut self, ip: IpAddr, domain: &str) {
        if self.ip_domain.len() >= self.cfg.ip_cache_cap && !self.ip_domain.contains_key(&ip) {
            // Простейшая эвикция: убрать произвольный элемент.
            if let Some(k) = self.ip_domain.keys().next().copied() {
                self.ip_domain.remove(&k);
            }
        }
        self.ip_domain.insert(ip, domain.to_string());
    }

    /// Выкидывает один наименее ценный поток (не-armed, самый старый).
    fn evict_one(&mut self) {
        // Предпочитаем снять handshake-поток; среди них — с наименьшим last_seen.
        let victim = self
            .flows
            .iter()
            .filter(|(_, f)| matches!(f.phase, Phase::Handshake))
            .min_by_key(|(_, f)| f.last_seen_ms)
            .map(|(k, _)| *k)
            .or_else(|| {
                self.flows
                    .iter()
                    .min_by_key(|(_, f)| f.last_seen_ms)
                    .map(|(k, _)| *k)
            });
        if let Some(k) = victim {
            self.flows.remove(&k);
        }
    }
}

// --- seq-сравнения с учётом wrap (RFC 1982 style) ---

fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

fn seq_le(a: u32, b: u32) -> bool {
    a == b || seq_lt(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eyes::parse::{FlowKey, TcpFlags};
    use std::net::{IpAddr, Ipv4Addr};

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));
    const LPORT: u16 = 51000;

    fn key() -> FlowKey {
        FlowKey {
            local_port: LPORT,
            remote_ip: IP,
            remote_port: 443,
        }
    }

    fn cfg() -> Config {
        Config {
            hostlist: vec!["youtube.com".into()],
            ..Config::default()
        }
    }

    fn syn(seq: u32) -> ParsedPacket {
        ParsedPacket {
            outbound: true,
            key: key(),
            ttl: 128,
            seq,
            flags: TcpFlags {
                syn: true,
                ..Default::default()
            },
            payload: vec![],
        }
    }

    fn synack() -> ParsedPacket {
        ParsedPacket {
            outbound: false,
            key: key(),
            ttl: 64,
            seq: 5000,
            flags: TcpFlags {
                syn: true,
                ack: true,
                ..Default::default()
            },
            payload: vec![],
        }
    }

    fn out_data(seq: u32, payload: Vec<u8>) -> ParsedPacket {
        ParsedPacket {
            outbound: true,
            key: key(),
            ttl: 128,
            seq,
            flags: TcpFlags {
                psh: true,
                ack: true,
                ..Default::default()
            },
            payload,
        }
    }

    fn in_rst() -> ParsedPacket {
        ParsedPacket {
            outbound: false,
            key: key(),
            ttl: 64,
            seq: 5001,
            flags: TcpFlags {
                rst: true,
                ..Default::default()
            },
            payload: vec![],
        }
    }

    fn in_data(payload: Vec<u8>) -> ParsedPacket {
        ParsedPacket {
            outbound: false,
            key: key(),
            ttl: 64,
            seq: 5001,
            flags: TcpFlags {
                psh: true,
                ack: true,
                ..Default::default()
            },
            payload,
        }
    }

    // Помощник из parse::tests недоступен здесь — соберём CH локально через parse-хелпер.
    fn client_hello(sni: &str) -> Vec<u8> {
        // Переиспользуем ту же раскладку, что и в parse::tests.
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
        body.extend_from_slice(&[0u8; 32]);
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

    fn server_hello() -> Vec<u8> {
        vec![0x16, 0x03, 0x03, 0x00, 0x04, 0x02, 0x00, 0x00, 0x00]
    }

    #[test]
    fn working_path_emits_working() {
        let mut t = FlowTable::new(cfg());
        assert!(t.on_packet(&syn(1000), 0).is_none());
        assert!(t.on_packet(&synack(), 10).is_none());
        assert!(t
            .on_packet(&out_data(1001, client_hello("www.youtube.com")), 20)
            .is_none());
        let obs = t.on_packet(&in_data(server_hello()), 40).unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.domain, "www.youtube.com");
        assert_eq!(obs.local_port, LPORT);
    }

    #[test]
    fn reset_path_emits_reset() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        let obs = t.on_packet(&in_rst(), 30).unwrap();
        assert_eq!(obs.verdict, Verdict::Reset);
        assert_eq!(obs.evidence, "inbound_rst");
    }

    #[test]
    fn reset_before_arm_is_silent() {
        // RST по потоку без распознанного SNI не должен давать вердикт.
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        assert!(t.on_packet(&in_rst(), 10).is_none());
    }

    #[test]
    fn hostlist_miss_is_dropped() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("example.com")), 20);
        // Поток должен быть выкинут: RST не даёт вердикта.
        assert!(t.on_packet(&in_rst(), 30).is_none());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn split_client_hello_reassembles_reversed() {
        // ClientHello, разбитый на 2 сегмента и пришедший в обратном порядке.
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        let ch = client_hello("www.youtube.com");
        let (a, b) = ch.split_at(ch.len() / 2);
        // Сначала второй сегмент (disorder), потом первый.
        t.on_packet(&out_data(1001 + a.len() as u32, b.to_vec()), 20);
        t.on_packet(&out_data(1001, a.to_vec()), 21);
        let obs = t.on_packet(&in_data(server_hello()), 40).unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.domain, "www.youtube.com");
    }

    #[test]
    fn fake_low_ttl_clienthello_is_ignored() {
        // Фейковый CH к «плохому» SNI с низким TTL не должен ни арсить, ни ломать поток.
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        let mut fake = out_data(1001, client_hello("fake-sni.com"));
        fake.ttl = 4; // инъекция winws
        assert!(t.on_packet(&fake, 15).is_none());
        // Реальный CH проходит и арсит.
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        let obs = t.on_packet(&in_data(server_hello()), 40).unwrap();
        assert_eq!(obs.domain, "www.youtube.com");
    }

    #[test]
    fn syn_blackhole_needs_learned_ip() {
        let mut t = FlowTable::new(cfg());
        // Первое соединение армится и учит IP→domain, потом Working.
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        t.on_packet(&in_data(server_hello()), 40);

        // Новое соединение к тому же IP: SYN + ретрансмиты, нет SYN-ACK.
        let k2 = FlowKey {
            local_port: 51001,
            remote_ip: IP,
            remote_port: 443,
        };
        let mk_syn = |seq, port| ParsedPacket {
            outbound: true,
            key: FlowKey {
                local_port: port,
                remote_ip: IP,
                remote_port: 443,
            },
            ttl: 128,
            seq,
            flags: TcpFlags {
                syn: true,
                ..Default::default()
            },
            payload: vec![],
        };
        let _ = k2;
        t.on_packet(&mk_syn(2000, 51001), 1000);
        t.on_packet(&mk_syn(2000, 51001), 2000); // retx 1
        t.on_packet(&mk_syn(2000, 51001), 3000); // retx 2
        let verdicts = t.on_tick(5000);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].verdict, Verdict::Blackhole);
        assert_eq!(verdicts[0].evidence, "syn_no_synack");
        assert_eq!(verdicts[0].domain, "www.youtube.com");
    }

    #[test]
    fn armed_silence_with_retransmit_is_blackhole() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        // Ретрансмит того же CH (тот же seq) — признак, что ответа нет.
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 1020);
        let verdicts = t.on_tick(6000);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].verdict, Verdict::Blackhole);
        assert_eq!(verdicts[0].evidence, "silence+retransmit");
    }

    #[test]
    fn done_flow_swallows_duplicates_then_evicts() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        t.on_packet(&in_data(server_hello()), 40);
        // Поздний дубль не даёт нового вердикта.
        assert!(t.on_packet(&in_data(server_hello()), 50).is_none());
        // После linger поток снимается.
        let _ = t.on_tick(3000);
        assert_eq!(t.len(), 0);
    }
}
