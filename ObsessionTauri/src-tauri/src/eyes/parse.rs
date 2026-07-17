//! Чистый разбор пакетов: TCP-флаги + TLS-record + SNI из ClientHello + детект ServerHello.
//! Никакой зависимости от WinDivert/Tauri — вход `ParsedPacket`, выход — факты о TLS.
//! Полный TLS-стек не нужен: читаем ровно столько байт, сколько нужно для сигнала.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Ключ потока. Нормализован к «локальный/удалённый» (а не src/dst), чтобы
/// входящие и исходящие пакеты одного соединения сходились в один ключ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
}

/// TCP-флаги, которые нас интересуют.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub rst: bool,
    pub fin: bool,
    pub psh: bool,
}

/// Плоское представление пакета, которое `capture` готовит из сырья WinDivert,
/// а `flow` затем скармливает в автомат. Направление берём из WinDivert address,
/// НЕ угадываем по портам.
#[derive(Clone, Debug)]
pub struct ParsedPacket {
    pub outbound: bool,
    pub key: FlowKey,
    pub ttl: u8,
    pub seq: u32,
    pub flags: TcpFlags,
    /// TCP-payload (без заголовков). Пустой для чистых SYN/ACK/RST.
    pub payload: Vec<u8>,
}

/// Тип TLS-хендшейка, если пакет несёт TLS-record с рукопожатием.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsHandshake {
    ClientHello,
    ServerHello,
}

/// Классификация payload по первому TLS-record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsRecord {
    /// Handshake (0x16) с распознанным типом.
    Handshake(TlsHandshake),
    /// Application Data (0x17) — соединение уже живое, шифрованные данные.
    AppData,
    /// Alert (0x15) — часто сопровождает разрыв.
    Alert,
    /// TLS-record есть, но не из тех, что нам важны (или не распарсился).
    Other,
    /// Это не похоже на TLS-record вовсе.
    NotTls,
}

/// Классифицирует начало payload как TLS-record.
/// Заголовок record: [content_type(1)][version(2)][length(2)][...].
pub fn classify_record(payload: &[u8]) -> TlsRecord {
    if payload.len() < 5 {
        return TlsRecord::NotTls;
    }
    // version major должен быть 0x03 (TLS 1.0-1.3 все используют 0x03xx на record-слое).
    if payload[1] != 0x03 {
        return TlsRecord::NotTls;
    }
    match payload[0] {
        0x16 => match handshake_type(payload) {
            Some(0x01) => TlsRecord::Handshake(TlsHandshake::ClientHello),
            Some(0x02) => TlsRecord::Handshake(TlsHandshake::ServerHello),
            _ => TlsRecord::Other,
        },
        0x17 => TlsRecord::AppData,
        0x15 => TlsRecord::Alert,
        0x14 | 0x18 => TlsRecord::Other, // ChangeCipherSpec / Heartbeat
        _ => TlsRecord::NotTls,
    }
}

/// Тип handshake-сообщения (байт сразу после 5-байтного record-заголовка).
fn handshake_type(payload: &[u8]) -> Option<u8> {
    payload.get(5).copied()
}

/// Извлекает SNI (server_name) из ClientHello.
/// Возвращает None, если это не ClientHello, данных не хватает, или расширения SNI нет.
///
/// Раскладка ClientHello после 5-байтного record-заголовка:
///   handshake_type(1)=0x01 | length(3) | version(2) | random(32)
///   | session_id: len(1)+data | cipher_suites: len(2)+data
///   | compression: len(1)+data | extensions: len(2)+data
/// Внутри extensions ищем тип 0x0000 (server_name) → SNI list → host_name(0x00).
pub fn extract_sni(payload: &[u8]) -> Option<String> {
    // Должен быть handshake-record с ClientHello.
    if classify_record(payload) != TlsRecord::Handshake(TlsHandshake::ClientHello) {
        return None;
    }
    // Тело хендшейка идёт после 5-байтного record-заголовка.
    let hs = &payload[5..];
    // hs[0]=0x01 (тип), hs[1..4]=длина. Дальше — тело ClientHello.
    let mut p: usize = 4; // пропускаем handshake type(1) + length(3)

    p = skip_fixed(hs, p, 2)?; // client_version
    p = skip_fixed(hs, p, 32)?; // random

    p = skip_vec(hs, p, 1)?; // session_id (len: 1 байт)
    p = skip_vec(hs, p, 2)?; // cipher_suites (len: 2 байта)
    p = skip_vec(hs, p, 1)?; // compression_methods (len: 1 байт)

    // extensions: total length(2), затем список расширений.
    let ext_total = read_u16(hs, p)?;
    p += 2;
    let ext_end = p.checked_add(ext_total as usize)?;
    if ext_end > hs.len() {
        return None;
    }

    while p + 4 <= ext_end {
        let ext_type = read_u16(hs, p)?;
        let ext_len = read_u16(hs, p + 2)? as usize;
        let body = p + 4;
        let body_end = body.checked_add(ext_len)?;
        if body_end > ext_end {
            return None;
        }
        if ext_type == 0x0000 {
            return parse_sni_extension(&hs[body..body_end]);
        }
        p = body_end;
    }
    None
}

/// Разбирает тело расширения server_name → первый host_name.
/// Раскладка: server_name_list_len(2) | [ name_type(1) | name_len(2) | name ]...
fn parse_sni_extension(ext: &[u8]) -> Option<String> {
    let list_len = read_u16(ext, 0)? as usize;
    let list_end = 2usize.checked_add(list_len)?.min(ext.len());
    let mut p = 2;
    while p + 3 <= list_end {
        let name_type = ext[p];
        let name_len = read_u16(ext, p + 1)? as usize;
        let name_start = p + 3;
        let name_end = name_start.checked_add(name_len)?;
        if name_end > ext.len() {
            return None;
        }
        if name_type == 0x00 {
            // host_name
            return std::str::from_utf8(&ext[name_start..name_end])
                .ok()
                .map(|s| s.to_ascii_lowercase());
        }
        p = name_end;
    }
    None
}

// --- низкоуровневые помощники (все bounds-safe, без паник) ---

fn read_u16(buf: &[u8], at: usize) -> Option<u16> {
    let b = buf.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// Сдвиг на `n` байт с проверкой границ.
fn skip_fixed(buf: &[u8], at: usize, n: usize) -> Option<usize> {
    let next = at.checked_add(n)?;
    if next > buf.len() {
        None
    } else {
        Some(next)
    }
}

/// Пропускает vector с префиксом длины: `len_bytes` (1 или 2) + тело.
fn skip_vec(buf: &[u8], at: usize, len_bytes: usize) -> Option<usize> {
    let len = match len_bytes {
        1 => *buf.get(at)? as usize,
        2 => read_u16(buf, at)? as usize,
        _ => return None,
    };
    let next = at.checked_add(len_bytes)?.checked_add(len)?;
    if next > buf.len() {
        None
    } else {
        Some(next)
    }
}

/// Разбирает сырой сетевой пакет (IPv4/IPv6 + TCP) из буфера WinDivert в
/// [`ParsedPacket`]. `outbound` приходит снаружи — из поля направления
/// `WINDIVERT_ADDRESS` (`Outbound`), сами по портам НЕ угадываем.
///
/// Возвращает None для не-TCP, усечённых или неподдерживаемых пакетов —
/// наблюдатель просто пропускает такое, не падая.
pub fn decode_ip_tcp(buf: &[u8], outbound: bool) -> Option<ParsedPacket> {
    let version = buf.first()? >> 4;
    match version {
        4 => decode_ipv4_tcp(buf, outbound),
        6 => decode_ipv6_tcp(buf, outbound),
        _ => None,
    }
}

fn decode_ipv4_tcp(buf: &[u8], outbound: bool) -> Option<ParsedPacket> {
    // IPv4-заголовок: IHL в младших 4 битах первого байта (в 32-битных словах).
    let ihl = (buf.first()? & 0x0f) as usize * 4;
    if ihl < 20 || buf.len() < ihl {
        return None;
    }
    if *buf.get(9)? != 6 {
        return None; // не TCP
    }
    let src = Ipv4Addr::new(*buf.get(12)?, *buf.get(13)?, *buf.get(14)?, *buf.get(15)?);
    let dst = Ipv4Addr::new(*buf.get(16)?, *buf.get(17)?, *buf.get(18)?, *buf.get(19)?);
    decode_tcp(
        buf,
        ihl,
        IpAddr::V4(src),
        IpAddr::V4(dst),
        *buf.get(8)?, // TTL
        outbound,
    )
}

fn decode_ipv6_tcp(buf: &[u8], outbound: bool) -> Option<ParsedPacket> {
    // Фиксированный IPv6-заголовок — 40 байт. Расширенные заголовки не разбираем:
    // для TLS-трафика их обычно нет; если next_header != TCP — пропускаем пакет.
    const HDR: usize = 40;
    if buf.len() < HDR {
        return None;
    }
    if *buf.get(6)? != 6 {
        return None; // next header != TCP (extension headers не поддерживаем)
    }
    let hop_limit = *buf.get(7)?; // аналог TTL
    let src: [u8; 16] = buf.get(8..24)?.try_into().ok()?;
    let dst: [u8; 16] = buf.get(24..40)?.try_into().ok()?;
    decode_tcp(
        buf,
        HDR,
        IpAddr::V6(Ipv6Addr::from(src)),
        IpAddr::V6(Ipv6Addr::from(dst)),
        hop_limit,
        outbound,
    )
}

/// Общий разбор TCP-сегмента поверх уже определённых src/dst/ttl.
fn decode_tcp(
    buf: &[u8],
    l4_off: usize,
    src_ip: IpAddr,
    dst_ip: IpAddr,
    ttl: u8,
    outbound: bool,
) -> Option<ParsedPacket> {
    let tcp = buf.get(l4_off..)?;
    if tcp.len() < 20 {
        return None;
    }
    let src_port = u16::from_be_bytes([tcp[0], tcp[1]]);
    let dst_port = u16::from_be_bytes([tcp[2], tcp[3]]);
    let seq = u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]);
    let data_off = (tcp[12] >> 4) as usize * 4;
    if data_off < 20 || tcp.len() < data_off {
        return None;
    }
    let f = tcp[13];
    let flags = TcpFlags {
        fin: f & 0x01 != 0,
        syn: f & 0x02 != 0,
        rst: f & 0x04 != 0,
        psh: f & 0x08 != 0,
        ack: f & 0x10 != 0,
    };
    let payload = tcp.get(data_off..).unwrap_or(&[]).to_vec();

    // Нормализация к local/remote по направлению: для исходящего локальным
    // является источник, для входящего — назначение.
    let (local_port, remote_ip, remote_port) = if outbound {
        (src_port, dst_ip, dst_port)
    } else {
        (dst_port, src_ip, src_port)
    };

    Some(ParsedPacket {
        outbound,
        key: FlowKey {
            local_port,
            remote_ip,
            remote_port,
        },
        ttl,
        seq,
        flags,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный ClientHello-record с заданным SNI.
    fn client_hello_with_sni(sni: &str) -> Vec<u8> {
        let host = sni.as_bytes();
        // server_name extension body
        let mut sni_ext = Vec::new();
        let name_len = host.len() as u16;
        let list_len = 3 + name_len; // name_type(1)+name_len(2)+name
        sni_ext.extend_from_slice(&list_len.to_be_bytes());
        sni_ext.push(0x00); // host_name
        sni_ext.extend_from_slice(&name_len.to_be_bytes());
        sni_ext.extend_from_slice(host);

        // extension: type(2)=0x0000 + len(2) + body
        let mut exts = Vec::new();
        exts.extend_from_slice(&0x0000u16.to_be_bytes());
        exts.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
        exts.extend_from_slice(&sni_ext);

        // тело ClientHello
        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]); // client_version TLS1.2
        body.extend_from_slice(&[0u8; 32]); // random
        body.push(0x00); // session_id len = 0
        body.extend_from_slice(&2u16.to_be_bytes()); // cipher_suites len
        body.extend_from_slice(&[0x13, 0x01]); // один cipher
        body.push(0x01); // compression len
        body.push(0x00); // null compression
        body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        body.extend_from_slice(&exts);

        // handshake: type(1)=0x01 + length(3)
        let mut hs = Vec::new();
        hs.push(0x01);
        let l = body.len() as u32;
        hs.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
        hs.extend_from_slice(&body);

        // record: type(1)=0x16 + version(2) + length(2)
        let mut rec = Vec::new();
        rec.push(0x16);
        rec.extend_from_slice(&[0x03, 0x01]);
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        rec
    }

    #[test]
    fn extracts_plain_sni() {
        let ch = client_hello_with_sni("www.youtube.com");
        assert_eq!(extract_sni(&ch).as_deref(), Some("www.youtube.com"));
    }

    #[test]
    fn sni_is_lowercased() {
        let ch = client_hello_with_sni("Discord.COM");
        assert_eq!(extract_sni(&ch).as_deref(), Some("discord.com"));
    }

    #[test]
    fn server_hello_classified() {
        // record 0x16, version, len, затем handshake type 0x02
        let rec = [0x16, 0x03, 0x03, 0x00, 0x04, 0x02, 0x00, 0x00, 0x00];
        assert_eq!(
            classify_record(&rec),
            TlsRecord::Handshake(TlsHandshake::ServerHello)
        );
    }

    #[test]
    fn app_data_classified() {
        let rec = [0x17, 0x03, 0x03, 0x00, 0x10];
        assert_eq!(classify_record(&rec), TlsRecord::AppData);
    }

    #[test]
    fn non_tls_rejected() {
        assert_eq!(classify_record(b"GET / HTTP/1.1\r\n"), TlsRecord::NotTls);
        assert_eq!(classify_record(&[0x16, 0x00, 0x00]), TlsRecord::NotTls);
    }

    #[test]
    fn truncated_client_hello_returns_none_not_panic() {
        let ch = client_hello_with_sni("example.com");
        for cut in 0..ch.len() {
            // Не должно паниковать ни на одном обрезке.
            let _ = extract_sni(&ch[..cut]);
        }
    }

    #[test]
    fn garbage_never_panics() {
        let inputs: [&[u8]; 4] = [&[], &[0x16], &[0x16, 0x03, 0x03, 0xff, 0xff], &[0u8; 8]];
        for i in inputs {
            let _ = classify_record(i);
            let _ = extract_sni(i);
        }
    }

    /// Собирает IPv4/TCP-пакет: 20б IP-заголовок + 20б TCP + payload.
    fn ipv4_tcp(
        endpoints: ([u8; 4], [u8; 4]),
        ports: (u16, u16),
        ttl: u8,
        seq: u32,
        syn: bool,
        payload: &[u8],
    ) -> Vec<u8> {
        let (src, dst) = endpoints;
        let (sport, dport) = ports;
        let mut p = Vec::new();
        // IP header
        p.push(0x45); // version 4, IHL 5
        p.push(0x00); // DSCP/ECN
        let total = (40 + payload.len()) as u16;
        p.extend_from_slice(&total.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]); // id, flags/frag
        p.push(ttl);
        p.push(6); // protocol TCP
        p.extend_from_slice(&[0, 0]); // checksum (не проверяется)
        p.extend_from_slice(&src);
        p.extend_from_slice(&dst);
        // TCP header
        p.extend_from_slice(&sport.to_be_bytes());
        p.extend_from_slice(&dport.to_be_bytes());
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]); // ack
        p.push(0x50); // data offset 5, reserved
        p.push(if syn { 0x02 } else { 0x18 }); // SYN, либо PSH+ACK
        p.extend_from_slice(&[0, 0]); // window
        p.extend_from_slice(&[0, 0, 0, 0]); // checksum, urgent
        p.extend_from_slice(payload);
        p
    }

    #[test]
    fn decode_outbound_normalizes_local_remote() {
        let pkt = ipv4_tcp(
            ([192, 168, 0, 5], [203, 0, 113, 9]),
            (51000, 443),
            128,
            1000,
            false,
            b"hello",
        );
        let d = decode_ip_tcp(&pkt, true).unwrap();
        assert!(d.outbound);
        assert_eq!(d.key.local_port, 51000);
        assert_eq!(d.key.remote_port, 443);
        assert_eq!(d.key.remote_ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)));
        assert_eq!(d.ttl, 128);
        assert_eq!(d.seq, 1000);
        assert_eq!(d.payload, b"hello");
        assert!(d.flags.psh && d.flags.ack);
    }

    #[test]
    fn decode_inbound_maps_to_same_flow_key() {
        // Ответный пакет того же соединения: src/dst перевёрнуты, direction=inbound.
        let pkt = ipv4_tcp(
            ([203, 0, 113, 9], [192, 168, 0, 5]),
            (443, 51000),
            54,
            5000,
            false,
            b"",
        );
        let d = decode_ip_tcp(&pkt, false).unwrap();
        assert!(!d.outbound);
        // Ключ должен совпасть с исходящим: local=51000, remote=203.0.113.9:443.
        assert_eq!(d.key.local_port, 51000);
        assert_eq!(d.key.remote_port, 443);
        assert_eq!(d.key.remote_ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)));
    }

    #[test]
    fn decode_rejects_non_tcp_and_truncated() {
        // UDP (protocol 17) — не наш.
        let mut udp = ipv4_tcp(([1, 1, 1, 1], [2, 2, 2, 2]), (1, 2), 64, 0, false, b"");
        udp[9] = 17;
        assert!(decode_ip_tcp(&udp, true).is_none());
        // Усечённые буферы не паникуют.
        let full = ipv4_tcp(([1, 1, 1, 1], [2, 2, 2, 2]), (1, 2), 64, 0, true, b"x");
        for cut in 0..full.len() {
            let _ = decode_ip_tcp(&full[..cut], true);
        }
    }
}
