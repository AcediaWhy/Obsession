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
use crate::eyes::parse::{classify_record, extract_sni, ParsedPacket, TlsHandshake, TlsRecord};
use crate::eyes::signal::{Observation, Verdict};

pub const ARMED_SILENCE_TIMEOUT_MS: u64 = 8_000;
pub const BLACKHOLE_DELIVERY_GRACE_MS: u64 = 2_000;

/// How inbound payload proves that an armed flow is working.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkingSignalMode {
    /// Preserve the original Eyes behavior used by Zapret2: every non-empty
    /// inbound payload except a recognized TLS Alert is a positive signal.
    #[default]
    Compatibility,
    /// Require a complete, length-valid TLS ServerHello or non-empty AppData
    /// record reconstructed from the inbound TCP stream.
    StrictTls,
}

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
    /// Задержка перед публикацией timeout-verdict, чтобы уже захваченный
    /// входящий пакет успел отменить ложный blackhole.
    pub blackhole_delivery_grace_ms: u64,
    /// Сколько держать «завершённый» поток, чтобы гасить поздние дубли пакетов.
    pub done_linger_ms: u64,
    /// Мин. число ретрансмитов SYN, чтобы счесть молчание блэкхолом (а не отменой).
    pub min_syn_retx: u8,
    /// Мин. число ретрансмитов ClientHello для blackhole в Armed.
    pub min_ch_retx: u8,
    /// Ёмкость кэша IP→domain (для атрибуции SYN-level blackhole).
    pub ip_cache_cap: usize,
    /// Семантика положительного сигнала для входящего TCP payload.
    pub working_signal_mode: WorkingSignalMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hostlist: Vec::new(),
            max_flows: 4096,
            syn_synack_timeout_ms: 3000,
            // Медленная мобильная/загруженная сеть может отдать первый ServerHello
            // не мгновенно — даём запас, чтобы «медленно, но работает» не читалось
            // как blackhole. Ложные заморозки дороже небольшой задержки детекта.
            armed_silence_timeout_ms: ARMED_SILENCE_TIMEOUT_MS,
            blackhole_delivery_grace_ms: 0,
            done_linger_ms: 2000,
            // Ретрансмиты — главная улика «сервер молчит». Один ретрансмит бывает и
            // на здоровом, но потерянном на линке пакете, поэтому требуем ≥3 (SYN)
            // и ≥2 (ClientHello): это отсекает единичные сетевые потери.
            min_syn_retx: 3,
            min_ch_retx: 2,
            ip_cache_cap: 1024,
            working_signal_mode: WorkingSignalMode::Compatibility,
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
    Armed {
        domain: String,
        t0: u64,
        capture_timestamp: Option<i64>,
    },
    /// Вердикт уже вынесен — держим недолго, чтобы гасить поздние дубли.
    Done,
}

#[derive(Clone, Copy, Debug)]
enum PendingBlackholeKind {
    SynNoSynAck,
    TlsSilence,
}

#[derive(Clone, Debug)]
struct PendingBlackhole {
    kind: PendingBlackholeKind,
    observation: Observation,
    publish_at_ms: u64,
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

/// Bounded inbound TCP prefix used only by [`WorkingSignalMode::StrictTls`].
///
/// A SYN-ACK anchors the first data sequence when it was observed. Without a
/// SYN-ACK (capture started mid-flow), the lowest observed payload sequence is
/// used, which still permits deterministic out-of-order reconstruction.
#[derive(Default)]
struct InboundReasm {
    base: Option<u32>,
    anchored: bool,
    segs: BTreeMap<u32, Vec<u8>>,
    stored: usize,
    conflicted: bool,
}

impl InboundReasm {
    const CAP: usize = 32 * 1024;
    const MAX_SEGMENTS: usize = 128;

    fn anchor_after_syn_ack(&mut self, syn_ack_seq: u32) {
        if self.base.is_none() {
            self.base = Some(syn_ack_seq.wrapping_add(1));
            self.anchored = true;
        }
    }

    fn insert(&mut self, seq: u32, data: &[u8]) -> bool {
        if data.is_empty() || self.conflicted || self.stored >= Self::CAP {
            return false;
        }

        match self.base {
            None => self.base = Some(seq),
            Some(base) if !self.anchored && seq_lt(seq, base) => self.base = Some(seq),
            _ => {}
        }
        let base = self.base.expect("base set above");

        let mut start = seq;
        let mut bytes = data;
        if seq_lt(start, base) {
            let end = start.wrapping_add(bytes.len() as u32);
            if seq_le(end, base) {
                return false;
            }
            let overlap = base.wrapping_sub(start) as usize;
            bytes = &bytes[overlap..];
            start = base;
        }

        let offset = start.wrapping_sub(base) as usize;
        if offset >= Self::CAP {
            return false;
        }
        let keep = bytes.len().min(Self::CAP - offset);
        if keep == 0 {
            return false;
        }
        let candidate = &bytes[..keep];
        let candidate_end = offset + candidate.len();
        let mut touched = Vec::new();
        let mut conflict = false;
        let mut merged_start = offset;
        let mut merged_end = candidate_end;

        for (&existing_seq, existing) in &self.segs {
            let existing_start = existing_seq.wrapping_sub(base) as usize;
            if existing_start >= Self::CAP {
                continue;
            }
            let existing_end = (existing_start + existing.len()).min(Self::CAP);
            if existing_end < offset || candidate_end < existing_start {
                continue;
            }

            let overlap_start = existing_start.max(offset);
            let overlap_end = existing_end.min(candidate_end);
            if overlap_start < overlap_end
                && existing[overlap_start - existing_start..overlap_end - existing_start]
                    != candidate[overlap_start - offset..overlap_end - offset]
            {
                conflict = true;
                break;
            }
            touched.push(existing_seq);
            merged_start = merged_start.min(existing_start);
            merged_end = merged_end.max(existing_end);
        }
        if conflict {
            self.conflicted = true;
            return false;
        }
        if touched.is_empty() && self.segs.len() >= Self::MAX_SEGMENTS {
            return false;
        }

        let removed_len = touched
            .iter()
            .filter_map(|key| self.segs.get(key))
            .map(Vec::len)
            .sum::<usize>();
        let merged_len = merged_end - merged_start;
        if merged_len <= removed_len {
            return false;
        }

        let mut merged = vec![0u8; merged_len];
        for key in &touched {
            let existing = self.segs.get(key).expect("touched segment remains present");
            let existing_start = key.wrapping_sub(base) as usize;
            let copy_len = existing
                .len()
                .min(merged_end.saturating_sub(existing_start));
            let at = existing_start - merged_start;
            merged[at..at + copy_len].copy_from_slice(&existing[..copy_len]);
        }
        let candidate_at = offset - merged_start;
        merged[candidate_at..candidate_at + candidate.len()].copy_from_slice(candidate);

        for key in touched {
            if let Some(removed) = self.segs.remove(&key) {
                self.stored -= removed.len();
            }
        }
        let merged_seq = base.wrapping_add(merged_start as u32);
        self.stored += merged.len();
        self.segs.insert(merged_seq, merged);
        true
    }

    fn contiguous(&self) -> Vec<u8> {
        if self.conflicted {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut want = match self.base {
            Some(base) => base,
            None => return out,
        };

        while out.len() < Self::CAP {
            let mut advanced = false;
            for (&start, data) in &self.segs {
                let end = start.wrapping_add(data.len() as u32);
                if seq_le(start, want) && seq_lt(want, end) {
                    let offset = want.wrapping_sub(start) as usize;
                    let remaining = Self::CAP - out.len();
                    let bytes = &data[offset..];
                    let keep = bytes.len().min(remaining);
                    out.extend_from_slice(&bytes[..keep]);
                    want = want.wrapping_add(keep as u32);
                    advanced = keep != 0;
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

const TLS_RECORD_HEADER_LEN: usize = 5;
// RFC 8446 permits 2^14 bytes of plaintext plus bounded expansion. TLS 1.2
// permits up to 2048 bytes of ciphertext expansion, which is the wider limit.
const MAX_TLS_RECORD_PAYLOAD: usize = (1 << 14) + 2048;

fn compatibility_working_evidence(payload: &[u8]) -> Option<&'static str> {
    let record = classify_record(payload);
    let is_alive = matches!(
        record,
        TlsRecord::Handshake(TlsHandshake::ServerHello) | TlsRecord::AppData
    ) || !matches!(record, TlsRecord::Alert);
    is_alive.then_some("server_hello")
}

fn strict_tls_working_evidence(stream: &[u8]) -> Option<&'static str> {
    let mut record_at = 0usize;
    let mut handshake = Vec::new();
    let mut handshake_at = 0usize;

    while record_at < stream.len() {
        let header = stream.get(record_at..record_at + TLS_RECORD_HEADER_LEN)?;
        let content_type = header[0];
        if !matches!(content_type, 0x14..=0x18) || header[1] != 0x03 || header[2] > 0x04 {
            return None;
        }

        let payload_len = u16::from_be_bytes([header[3], header[4]]) as usize;
        if payload_len > MAX_TLS_RECORD_PAYLOAD {
            return None;
        }
        let payload_at = record_at.checked_add(TLS_RECORD_HEADER_LEN)?;
        let record_end = payload_at.checked_add(payload_len)?;
        let payload = stream.get(payload_at..record_end)?;

        match content_type {
            0x16 => {
                if handshake.len().checked_add(payload.len())? > InboundReasm::CAP {
                    return None;
                }
                handshake.extend_from_slice(payload);
                while handshake.len().saturating_sub(handshake_at) >= 4 {
                    let message_type = handshake[handshake_at];
                    let message_len = ((handshake[handshake_at + 1] as usize) << 16)
                        | ((handshake[handshake_at + 2] as usize) << 8)
                        | handshake[handshake_at + 3] as usize;
                    if message_len > InboundReasm::CAP - 4 {
                        return None;
                    }
                    let message_end = handshake_at.checked_add(4)?.checked_add(message_len)?;
                    if message_end > handshake.len() {
                        break;
                    }
                    if message_type == 0x02
                        && valid_server_hello_body(&handshake[handshake_at + 4..message_end])
                    {
                        return Some("server_hello");
                    }
                    handshake_at = message_end;
                }
            }
            0x17 if !payload.is_empty() => return Some("tls_app_data"),
            _ => {}
        }

        record_at = record_end;
    }
    None
}

/// Checks the bounded structural fields of a TLS ServerHello body. This is
/// intentionally not a full TLS parser, but a handshake type plus one byte is
/// not enough to prove a real ServerHello and would make block-page noise look
/// healthy.
fn valid_server_hello_body(body: &[u8]) -> bool {
    // legacy_version(2) + random(32) + session_id_len(1) + cipher(2) +
    // compression(1), with an optional extensions vector.
    if body.len() < 38 || body[0] != 0x03 || body[1] > 0x04 {
        return false;
    }
    let session_len = body[34] as usize;
    if session_len > 32 {
        return false;
    }
    let fixed_end = 35usize + session_len + 3;
    if fixed_end > body.len() {
        return false;
    }
    if fixed_end == body.len() {
        return true;
    }
    let Some(length) = body.get(fixed_end..fixed_end + 2) else {
        return false;
    };
    let extensions_len = u16::from_be_bytes([length[0], length[1]]) as usize;
    fixed_end
        .checked_add(2)
        .and_then(|start| start.checked_add(extensions_len))
        .is_some_and(|end| end == body.len())
}

/// Один отслеживаемый поток.
struct Flow {
    id: u64,
    key_ip: IpAddr,
    local_port: u16,
    remote_port: u16,
    phase: Phase,
    fake_ctx: FakeContext,
    reasm: Reasm,
    inbound_reasm: InboundReasm,
    created_ms: u64,
    last_seen_ms: u64,
    syn_seen: bool,
    syn_ack_seen: bool,
    syn_retx: u8,
    ch_retx: u8,
    first_payload_capture_timestamp: Option<i64>,
    inbound_data_seen: bool,
    pending_blackhole: Option<PendingBlackhole>,
    emitted: bool,
}

impl Flow {
    fn new(id: u64, key_ip: IpAddr, local_port: u16, remote_port: u16, now: u64) -> Self {
        Self {
            id,
            key_ip,
            local_port,
            remote_port,
            phase: Phase::Handshake,
            fake_ctx: FakeContext::default(),
            reasm: Reasm::default(),
            inbound_reasm: InboundReasm::default(),
            created_ms: now,
            last_seen_ms: now,
            syn_seen: false,
            syn_ack_seen: false,
            syn_retx: 0,
            ch_retx: 0,
            first_payload_capture_timestamp: None,
            inbound_data_seen: false,
            pending_blackhole: None,
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
    next_flow_id: u64,
}

impl FlowTable {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            flows: HashMap::new(),
            ip_domain: HashMap::new(),
            next_flow_id: 1,
        }
    }

    fn allocate_flow_id(&mut self) -> u64 {
        let id = self.next_flow_id;
        self.next_flow_id = self.next_flow_id.wrapping_add(1);
        if self.next_flow_id == 0 {
            self.next_flow_id = 1;
        }
        id
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.flows.len()
    }

    /// Обрабатывает один пакет. Возвращает вердикт, если он готов.
    pub fn on_packet(&mut self, pkt: &ParsedPacket, ts_ms: u64) -> Option<Observation> {
        self.on_packet_inner(pkt, ts_ms, None)
    }

    pub(crate) fn on_captured_packet(
        &mut self,
        pkt: &ParsedPacket,
        ts_ms: u64,
        capture_timestamp: i64,
    ) -> Option<Observation> {
        self.on_packet_inner(pkt, ts_ms, Some(capture_timestamp))
    }

    fn on_packet_inner(
        &mut self,
        pkt: &ParsedPacket,
        ts_ms: u64,
        capture_timestamp: Option<i64>,
    ) -> Option<Observation> {
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
            self.on_outbound(pkt, ts_ms, capture_timestamp)
        } else {
            self.on_inbound(pkt, ts_ms)
        }
    }

    fn on_outbound(
        &mut self,
        pkt: &ParsedPacket,
        now: u64,
        capture_timestamp: Option<i64>,
    ) -> Option<Observation> {
        let flags = pkt.flags;

        // Исходящий SYN (без ACK) — начало соединения / baseline TTL,seq.
        if flags.syn && !flags.ack {
            if self.flows.len() >= self.cfg.max_flows && !self.flows.contains_key(&pkt.key) {
                self.evict_one();
            }
            if !self.flows.contains_key(&pkt.key) {
                let flow_id = self.allocate_flow_id();
                self.flows.insert(
                    pkt.key,
                    Flow::new(
                        flow_id,
                        pkt.key.remote_ip,
                        pkt.key.local_port,
                        pkt.key.remote_port,
                        now,
                    ),
                );
            }
            // evict_one мог удалить только что вставленный ключ при совпадении —
            // на горячем пути пакетов это не паника, а пропуск (None → return).
            let f = self.flows.get_mut(&pkt.key)?;
            if f.syn_seen {
                f.syn_retx = f.syn_retx.saturating_add(1); // ретрансмит SYN
            } else {
                f.syn_seen = true;
                f.fake_ctx.note_syn(pkt.ttl, pkt.seq);
            }
            f.last_seen_ms = now;
            return None;
        }

        // Исходящий RST/FIN — соединение закрывает САМ клиент. Это не может быть
        // признаком сетевой блокировки: браузеры штатно бросают спекулятивные TCP
        // (happy-eyeballs / preconnect / выигрыш QUIC-vs-TCP), отправив FIN/RST.
        // Снимаем поток без вердикта, иначе тик «дозреет» до ложного blackhole
        // (`silence+retransmit` / `syn_no_synack`) по потоку, который мы же и
        // закрыли. Реальный blackhole персистентен: даст молчащие ОТКРЫТЫЕ потоки
        // (клиент ещё не сдался) либо новые Armed-потоки — их предохранитель поймает.
        if flags.rst || flags.fin {
            if let Some(f) = self.flows.get(&pkt.key) {
                if !f.emitted {
                    self.flows.remove(&pkt.key);
                }
            }
            return None;
        }

        // Исходящие данные — кандидат в ClientHello (или его фрагмент).
        if !pkt.payload.is_empty() {
            // Поток мог не иметь SYN (мы подключились в середине) — создаём лениво.
            if !self.flows.contains_key(&pkt.key) {
                if self.flows.len() >= self.cfg.max_flows {
                    self.evict_one();
                }
                let flow_id = self.allocate_flow_id();
                self.flows.insert(
                    pkt.key,
                    Flow::new(
                        flow_id,
                        pkt.key.remote_ip,
                        pkt.key.local_port,
                        pkt.key.remote_port,
                        now,
                    ),
                );
            }
            // evicted между insert и get — пропускаем пакет, не паникуем.
            let f = self.flows.get_mut(&pkt.key)?;
            f.last_seen_ms = now;
            if f.first_payload_capture_timestamp.is_none() {
                f.first_payload_capture_timestamp = capture_timestamp;
            }

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
                        capture_timestamp: f.first_payload_capture_timestamp,
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
        let working_signal_mode = self.cfg.working_signal_mode;
        let f = self.flows.get_mut(&pkt.key)?;
        f.last_seen_ms = now;

        // Входящий SYN-ACK — TCP встал, это уже не SYN-level blackhole.
        if pkt.flags.syn && pkt.flags.ack {
            f.syn_ack_seen = true;
            f.pending_blackhole = None;
            if working_signal_mode == WorkingSignalMode::StrictTls {
                f.inbound_reasm.anchor_after_syn_ack(pkt.seq);
            }
            return None;
        }

        // Входящий RST: DPI срезал попытку. Вердикт только если знаем домен (Armed).
        if pkt.flags.rst {
            if let Phase::Armed {
                domain,
                t0,
                capture_timestamp,
            } = &f.phase
            {
                let obs = Observation {
                    flow_id: f.id,
                    domain: domain.clone(),
                    dst_ip: f.key_ip,
                    local_port: f.local_port,
                    remote_port: f.remote_port,
                    verdict: Verdict::Reset,
                    evidence: "inbound_rst",
                    armed_at_ms: Some(*t0),
                    armed_at_capture_timestamp: *capture_timestamp,
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
            f.pending_blackhole = None;
            let evidence = match working_signal_mode {
                WorkingSignalMode::Compatibility => compatibility_working_evidence(&pkt.payload),
                WorkingSignalMode::StrictTls => {
                    f.inbound_reasm.insert(pkt.seq, &pkt.payload);
                    strict_tls_working_evidence(&f.inbound_reasm.contiguous())
                }
            };
            if let Some(evidence) = evidence {
                if let Phase::Armed {
                    domain,
                    t0,
                    capture_timestamp,
                } = &f.phase
                {
                    let obs = Observation {
                        flow_id: f.id,
                        domain: domain.clone(),
                        dst_ip: f.key_ip,
                        local_port: f.local_port,
                        remote_port: f.remote_port,
                        verdict: Verdict::Working,
                        evidence,
                        armed_at_ms: Some(*t0),
                        armed_at_capture_timestamp: *capture_timestamp,
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
            let Some(f) = self.flows.get_mut(&key) else {
                continue;
            };

            // Завершённые — снять после linger.
            if f.emitted {
                if now.saturating_sub(f.last_seen_ms) >= self.cfg.done_linger_ms {
                    to_remove.push(key);
                }
                continue;
            }

            if let Some(pending) = f.pending_blackhole.as_ref() {
                let still_adverse = match pending.kind {
                    PendingBlackholeKind::SynNoSynAck => {
                        !f.syn_ack_seen
                            && !f.inbound_data_seen
                            && f.syn_retx >= self.cfg.min_syn_retx
                    }
                    PendingBlackholeKind::TlsSilence => {
                        !f.inbound_data_seen && f.ch_retx >= self.cfg.min_ch_retx
                    }
                };
                if still_adverse {
                    if now >= pending.publish_at_ms {
                        out.push(pending.observation.clone());
                        to_remove.push(key);
                    }
                    continue;
                }
                f.pending_blackhole = None;
            }

            match &f.phase {
                Phase::Handshake => {
                    let age = now.saturating_sub(f.created_ms);
                    if age >= self.cfg.syn_synack_timeout_ms {
                        // SYN-level blackhole: нет SYN-ACK + ретрансмиты SYN.
                        if !f.syn_ack_seen
                            && !f.inbound_data_seen
                            && f.syn_retx >= self.cfg.min_syn_retx
                        {
                            // Домен неизвестен (SNI не видели) — берём из learned IP→domain.
                            if let Some(domain) = self.ip_domain.get(&f.key_ip).cloned() {
                                let observation = Observation {
                                    flow_id: f.id,
                                    domain,
                                    dst_ip: f.key_ip,
                                    local_port: f.local_port,
                                    remote_port: f.remote_port,
                                    verdict: Verdict::Blackhole,
                                    evidence: "syn_no_synack",
                                    armed_at_ms: None,
                                    armed_at_capture_timestamp: None,
                                    ts_ms: now,
                                };
                                if self.cfg.blackhole_delivery_grace_ms == 0 {
                                    out.push(observation);
                                    to_remove.push(key);
                                } else {
                                    f.pending_blackhole = Some(PendingBlackhole {
                                        kind: PendingBlackholeKind::SynNoSynAck,
                                        observation,
                                        publish_at_ms: now
                                            .saturating_add(self.cfg.blackhole_delivery_grace_ms),
                                    });
                                }
                            } else {
                                to_remove.push(key);
                            }
                        } else {
                            to_remove.push(key);
                        }
                    }
                }
                Phase::Armed {
                    domain,
                    t0,
                    capture_timestamp,
                } => {
                    let silent = now.saturating_sub(*t0);
                    if silent >= self.cfg.armed_silence_timeout_ms {
                        if !f.inbound_data_seen && f.ch_retx >= self.cfg.min_ch_retx {
                            let observation = Observation {
                                flow_id: f.id,
                                domain: domain.clone(),
                                dst_ip: f.key_ip,
                                local_port: f.local_port,
                                remote_port: f.remote_port,
                                verdict: Verdict::Blackhole,
                                evidence: "silence+retransmit",
                                armed_at_ms: Some(*t0),
                                armed_at_capture_timestamp: *capture_timestamp,
                                ts_ms: now,
                            };
                            if self.cfg.blackhole_delivery_grace_ms == 0 {
                                out.push(observation);
                                to_remove.push(key);
                            } else {
                                f.pending_blackhole = Some(PendingBlackhole {
                                    kind: PendingBlackholeKind::TlsSilence,
                                    observation,
                                    publish_at_ms: now
                                        .saturating_add(self.cfg.blackhole_delivery_grace_ms),
                                });
                            }
                        } else {
                            to_remove.push(key);
                        }
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

    fn strict_cfg() -> Config {
        Config {
            working_signal_mode: WorkingSignalMode::StrictTls,
            ..cfg()
        }
    }

    fn tentative_cfg() -> Config {
        Config {
            blackhole_delivery_grace_ms: 2_000,
            ..cfg()
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

    fn in_data_at(seq: u32, payload: Vec<u8>) -> ParsedPacket {
        ParsedPacket {
            outbound: false,
            key: key(),
            ttl: 64,
            seq,
            flags: TcpFlags {
                psh: true,
                ack: true,
                ..Default::default()
            },
            payload,
        }
    }

    fn in_data(payload: Vec<u8>) -> ParsedPacket {
        in_data_at(5001, payload)
    }

    fn out_fin() -> ParsedPacket {
        ParsedPacket {
            outbound: true,
            key: key(),
            ttl: 128,
            seq: 2000,
            flags: TcpFlags {
                fin: true,
                ack: true,
                ..Default::default()
            },
            payload: vec![],
        }
    }

    fn out_rst(port: u16) -> ParsedPacket {
        ParsedPacket {
            outbound: true,
            key: FlowKey {
                local_port: port,
                remote_ip: IP,
                remote_port: 443,
            },
            ttl: 128,
            seq: 2000,
            flags: TcpFlags {
                rst: true,
                ..Default::default()
            },
            payload: vec![],
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
        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[0u8; 32]);
        body.push(0); // session id length
        body.extend_from_slice(&[0x00, 0x2f]); // TLS_RSA_WITH_AES_128_CBC_SHA
        body.push(0); // null compression

        let mut handshake = vec![0x02, 0x00, 0x00, body.len() as u8];
        handshake.extend_from_slice(&body);
        let mut record = vec![0x16, 0x03, 0x03];
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);
        record
    }

    #[test]
    fn working_path_emits_working() {
        let mut t = FlowTable::new(cfg());
        assert!(t.on_packet(&syn(1000), 0).is_none());
        assert!(t.on_packet(&synack(), 10).is_none());
        assert!(t
            .on_captured_packet(&out_data(1001, client_hello("www.youtube.com")), 20, 2_000,)
            .is_none());
        let obs = t.on_packet(&in_data(server_hello()), 40).unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.domain, "www.youtube.com");
        assert_eq!(obs.local_port, LPORT);
        assert_ne!(obs.flow_id, 0);
        assert_eq!(obs.remote_port, 443);
        assert_eq!(obs.evidence, "server_hello");
        assert_eq!(obs.armed_at_ms, Some(20));
        assert_eq!(obs.armed_at_capture_timestamp, Some(2_000));
    }

    #[test]
    fn default_mode_preserves_compatibility_payload_semantics() {
        assert_eq!(
            Config::default().working_signal_mode,
            WorkingSignalMode::Compatibility
        );
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let obs = t
            .on_packet(&in_data(b"HTTP/1.1 302 Found\r\n\r\n".to_vec()), 40)
            .unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.evidence, "server_hello");
    }

    #[test]
    fn compatibility_still_rejects_a_recognized_tls_alert() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        assert!(t
            .on_packet(&in_data(vec![0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x28]), 40)
            .is_none());
    }

    #[test]
    fn strict_tls_rejects_non_tls_payload() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        assert!(t
            .on_packet(&in_data(b"HTTP/1.1 302 Found\r\n\r\n".to_vec()), 40)
            .is_none());
    }

    #[test]
    fn strict_tls_reassembles_server_hello_at_header_boundaries() {
        for split_at in [1usize, 4, 5] {
            let mut t = FlowTable::new(strict_cfg());
            t.on_packet(&syn(1000), 0);
            t.on_packet(&synack(), 10);
            t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

            let record = server_hello();
            let (first, second) = record.split_at(split_at);
            assert!(t.on_packet(&in_data_at(5001, first.to_vec()), 40).is_none());
            let obs = t
                .on_packet(&in_data_at(5001 + split_at as u32, second.to_vec()), 41)
                .unwrap();
            assert_eq!(obs.verdict, Verdict::Working);
            assert_eq!(obs.evidence, "server_hello");
        }
    }

    #[test]
    fn strict_tls_scans_coalesced_complete_records() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let mut records = vec![0x14, 0x03, 0x03, 0x00, 0x01, 0x01];
        records.extend_from_slice(&server_hello());
        let obs = t.on_packet(&in_data(records), 40).unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.evidence, "server_hello");
    }

    #[test]
    fn strict_tls_app_data_requires_its_declared_nonempty_body() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let header = vec![0x17, 0x03, 0x03, 0x00, 0x01];
        assert!(t.on_packet(&in_data_at(5001, header), 40).is_none());
        let obs = t.on_packet(&in_data_at(5006, vec![0x00]), 41).unwrap();
        assert_eq!(obs.verdict, Verdict::Working);
        assert_eq!(obs.evidence, "tls_app_data");
    }

    #[test]
    fn strict_tls_accepts_a_more_complete_same_seq_retransmission() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let record = vec![0x17, 0x03, 0x03, 0x00, 0x01, 0x2a];
        assert!(t
            .on_packet(&in_data_at(5001, record[..5].to_vec()), 40)
            .is_none());
        let obs = t.on_packet(&in_data_at(5001, record), 41).unwrap();
        assert_eq!(obs.evidence, "tls_app_data");
    }

    #[test]
    fn strict_tls_zero_length_app_data_is_not_working() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        assert!(t
            .on_packet(&in_data(vec![0x17, 0x03, 0x03, 0x00, 0x00]), 40)
            .is_none());
    }

    #[test]
    fn strict_tls_rejects_impossible_record_length() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let impossible = (MAX_TLS_RECORD_PAYLOAD + 1) as u16;
        let mut payload = vec![0x17, 0x03, 0x03];
        payload.extend_from_slice(&impossible.to_be_bytes());
        payload.extend_from_slice(&server_hello());
        assert!(t.on_packet(&in_data(payload), 40).is_none());
    }

    #[test]
    fn strict_tls_rejects_structurally_short_server_hello() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        let too_short = vec![0x16, 0x03, 0x03, 0x00, 0x05, 0x02, 0x00, 0x00, 0x01, 0x00];
        assert!(t.on_packet(&in_data(too_short), 40).is_none());
    }

    #[test]
    fn strict_tls_conflicting_overlap_never_becomes_working() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        assert!(t
            .on_packet(&in_data_at(5005, vec![0xff, 0xff]), 30)
            .is_none());
        assert!(t
            .on_packet(
                &in_data_at(5001, vec![0x17, 0x03, 0x03, 0x00, 0x01, 0x2a]),
                40,
            )
            .is_none());
        let flow = t.flows.get(&key()).unwrap();
        assert!(flow.inbound_reasm.conflicted);
        assert!(flow.inbound_reasm.contiguous().is_empty());
    }

    #[test]
    fn strict_tls_invalid_inbound_is_evicted_without_policy_verdict() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        let hello = client_hello("www.youtube.com");
        t.on_packet(&out_data(1001, hello.clone()), 20);
        t.on_packet(&out_data(1001, hello.clone()), 30);
        t.on_packet(&out_data(1001, hello), 40);
        assert!(t
            .on_packet(&in_data(vec![0x17, 0x03, 0x03, 0x00, 0x01]), 50)
            .is_none());

        assert!(t.on_tick(9_000).is_empty());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn strict_tls_inbound_reassembly_is_bounded_per_flow() {
        let mut t = FlowTable::new(strict_cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&synack(), 10);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);

        assert!(t
            .on_packet(&in_data(vec![0; InboundReasm::CAP * 2]), 40)
            .is_none());
        let flow = t.flows.get(&key()).unwrap();
        assert_eq!(flow.inbound_reasm.stored, InboundReasm::CAP);
        assert_eq!(flow.inbound_reasm.contiguous().len(), InboundReasm::CAP);
    }

    #[test]
    fn reset_path_emits_reset() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        let obs = t.on_packet(&in_rst(), 30).unwrap();
        assert_eq!(obs.verdict, Verdict::Reset);
        assert_eq!(obs.evidence, "inbound_rst");
        assert_ne!(obs.flow_id, 0);
        assert_eq!(obs.remote_port, 443);
        assert_eq!(obs.armed_at_ms, Some(20));
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
        t.on_packet(&mk_syn(2000, 51001), 4000); // retx 3 → syn_retx=3 (порог)
        let verdicts = t.on_tick(5000);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].verdict, Verdict::Blackhole);
        assert_eq!(verdicts[0].evidence, "syn_no_synack");
        assert_eq!(verdicts[0].domain, "www.youtube.com");
        assert_eq!(verdicts[0].armed_at_ms, None);
    }

    #[test]
    fn armed_silence_with_retransmit_is_blackhole() {
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_captured_packet(&out_data(1001, client_hello("www.youtube.com")), 20, 2_000);
        // Два ретрансмита того же CH (тот же seq) — сервер молчит (min_ch_retx=2).
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 1020);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 2020);
        let verdicts = t.on_tick(9000);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].verdict, Verdict::Blackhole);
        assert_eq!(verdicts[0].evidence, "silence+retransmit");
        assert_eq!(verdicts[0].armed_at_ms, Some(20));
        assert_eq!(verdicts[0].armed_at_capture_timestamp, Some(2_000));
    }

    #[test]
    fn queued_server_hello_cancels_tentative_tls_blackhole() {
        let mut t = FlowTable::new(tentative_cfg());
        let hello = client_hello("www.youtube.com");
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, hello.clone()), 20);
        t.on_packet(&out_data(1001, hello.clone()), 1_020);
        t.on_packet(&out_data(1001, hello), 2_020);

        assert!(t.on_tick(9_000).is_empty());
        let working = t.on_packet(&in_data(server_hello()), 9_001).unwrap();
        assert_eq!(working.verdict, Verdict::Working);
        assert!(t.on_tick(11_000).is_empty());
    }

    #[test]
    fn tentative_tls_blackhole_publishes_only_after_full_grace() {
        let mut t = FlowTable::new(tentative_cfg());
        let hello = client_hello("www.youtube.com");
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, hello.clone()), 20);
        t.on_packet(&out_data(1001, hello.clone()), 1_020);
        t.on_packet(&out_data(1001, hello), 2_020);

        assert!(t.on_tick(9_000).is_empty());
        assert!(t.on_tick(10_999).is_empty());
        let observations = t.on_tick(11_000);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].verdict, Verdict::Blackhole);
    }

    #[test]
    fn packet_at_tentative_publish_deadline_wins_over_blackhole() {
        let mut t = FlowTable::new(tentative_cfg());
        let hello = client_hello("www.youtube.com");
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, hello.clone()), 20);
        t.on_packet(&out_data(1001, hello.clone()), 1_020);
        t.on_packet(&out_data(1001, hello), 2_020);

        assert!(t.on_tick(9_000).is_empty());
        assert!(t.on_tick(10_999).is_empty());
        let working = t.on_packet(&in_data(server_hello()), 11_000).unwrap();
        assert_eq!(working.verdict, Verdict::Working);
        assert!(t.on_tick(11_000).is_empty());
    }

    #[test]
    fn late_synack_cancels_tentative_syn_blackhole() {
        let mut t = FlowTable::new(tentative_cfg());
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 0);
        t.on_packet(&out_fin(), 1);

        let second_port = LPORT + 1;
        for (index, sequence) in [2_000, 2_000, 2_000, 2_000].into_iter().enumerate() {
            let mut packet = syn(sequence);
            packet.key.local_port = second_port;
            t.on_packet(&packet, 100 + index as u64 * 500);
        }

        assert!(t.on_tick(5_000).is_empty());
        let mut late_synack = synack();
        late_synack.key.local_port = second_port;
        assert!(t.on_packet(&late_synack, 5_001).is_none());
        assert!(t.on_tick(7_000).is_empty());
    }

    #[test]
    fn outbound_fin_cancels_armed_flow_no_blackhole() {
        // Клиент САМ закрыл Armed-поток (бросил спекулятивный сокет / выиграл
        // QUIC). Завязка как в armed_silence (CH + 2 ретрансмита, порог достигнут),
        // но с исходящим FIN — ложного silence+retransmit blackhole быть не должно.
        let mut t = FlowTable::new(cfg());
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 1020); // ch_retx=1
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 2020); // ch_retx=2 (порог)
        assert!(t.on_packet(&out_fin(), 2100).is_none());
        assert_eq!(t.len(), 0, "Armed-поток снят при клиентском FIN");
        let verdicts = t.on_tick(9000);
        assert!(
            verdicts.is_empty(),
            "нет blackhole по потоку, который закрыл сам клиент"
        );
    }

    #[test]
    fn outbound_rst_cancels_handshake_no_syn_blackhole() {
        // SYN + ретрансмиты к УЧЁНОМУ IP (порог syn_no_synack достигнут), но клиент
        // сам шлёт RST (happy-eyeballs бросает проигравший сокет) → нет blackhole.
        let mut t = FlowTable::new(cfg());
        // Учим IP→domain первым рабочим соединением (порт 51000).
        t.on_packet(&syn(1000), 0);
        t.on_packet(&out_data(1001, client_hello("www.youtube.com")), 20);
        t.on_packet(&in_data(server_hello()), 40);
        // Второе соединение (порт 51001): SYN + 3 ретрансмита, затем клиентский RST.
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
        t.on_packet(&mk_syn(2000, 51001), 1000);
        t.on_packet(&mk_syn(2000, 51001), 2000); // retx1
        t.on_packet(&mk_syn(2000, 51001), 3000); // retx2
        t.on_packet(&mk_syn(2000, 51001), 4000); // retx3 → syn_retx=3 (порог)
        t.on_packet(&out_rst(51001), 4100); // клиент закрывает соединение
        let verdicts = t.on_tick(6000);
        assert!(
            verdicts.is_empty(),
            "клиентский RST снимает handshake — нет syn_no_synack blackhole"
        );
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
