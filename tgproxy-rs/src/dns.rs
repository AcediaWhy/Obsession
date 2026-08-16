//! Минимальный DNS-резолвер A-записей поверх UDP.
//!
//! Зачем: системный резолвер на цензурирующих провайдерах молча не
//! отвечает для CF-relay доменов, поэтому фолбэк ходит напрямую к
//! публичным резолверам (8.8.8.8, 1.1.1.1).

use std::net::Ipv4Addr;
use std::time::Duration;

use rand::RngCore;
use tokio::net::UdpSocket;

pub const PUBLIC_RESOLVERS: [&str; 2] = ["8.8.8.8", "1.1.1.1"];
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE: usize = 512;

/// Резолв через конкретный сервер. Возвращает первый A-ответ.
pub async fn resolve_via(host: &str, resolver: &str) -> Result<Ipv4Addr, String> {
    let query = build_query(host);
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|error| format!("bind: {error}"))?;
    socket
        .connect((resolver, 53))
        .await
        .map_err(|error| format!("connect {resolver}: {error}"))?;
    socket
        .send(&query)
        .await
        .map_err(|error| format!("send: {error}"))?;

    let mut buffer = vec![0u8; MAX_RESPONSE];
    let (len, _) = tokio::time::timeout(QUERY_TIMEOUT, socket.recv_from(&mut buffer))
        .await
        .map_err(|_| format!("{resolver}: timeout"))?
        .map_err(|error| format!("recv: {error}"))?;
    buffer.truncate(len);

    parse_a_answer(&query, &buffer)
}

/// Сначала системный резолвер, затем публичные — если провайдерский
/// DNS молчит или врёт.
pub async fn resolve(host: &str) -> Result<Ipv4Addr, String> {
    match tokio::net::lookup_host((host, 443)).await {
        Ok(addrs) => {
            for addr in addrs {
                if let std::net::IpAddr::V4(ip) = addr.ip() {
                    return Ok(ip);
                }
            }
        }
        Err(_) => {}
    }
    for resolver in PUBLIC_RESOLVERS {
        if let Ok(ip) = resolve_via(host, resolver).await {
            return Ok(ip);
        }
    }
    Err(format!("no A record for {host}"))
}

/// Запрос: header (RD=1) + QNAME + type A + class IN.
fn build_query(host: &str) -> Vec<u8> {
    let mut id = [0u8; 2];
    rand::rngs::OsRng.fill_bytes(&mut id);

    let mut query = Vec::with_capacity(host.len() + 18);
    query.extend_from_slice(&id);
    query.extend_from_slice(&[0x01, 0x00]); // RD
    query.extend_from_slice(&[0x00, 0x01]); // QDCOUNT
    query.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in host.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&[0x00, 0x01]); // A
    query.extend_from_slice(&[0x00, 0x01]); // IN
    query
}

/// Разбор ответа: сверка id, rcode, пропуск вопроса, первая A-запись.
/// Указатели сжатия имён (0xC0) пропускаются без разыменования —
/// сами имена нам не нужны.
fn parse_a_answer(query: &[u8], response: &[u8]) -> Result<Ipv4Addr, String> {
    if response.len() < 12 {
        return Err("truncated header".to_string());
    }
    if response[0..2] != query[0..2] {
        return Err("id mismatch".to_string());
    }
    let rcode = response[3] & 0x0f;
    if rcode != 0 {
        return Err(format!("rcode {rcode}"));
    }
    let answers = u16::from_be_bytes([response[6], response[7]]);
    if answers == 0 {
        return Err("no answers".to_string());
    }

    let mut offset = 12usize;
    // Вопрос: QNAME + TYPE + CLASS.
    offset = skip_name(response, offset)?;
    offset += 4;

    for _ in 0..answers {
        offset = skip_name(response, offset)?;
        if offset + 10 > response.len() {
            return Err("truncated answer".to_string());
        }
        let record_type = u16::from_be_bytes([response[offset], response[offset + 1]]);
        let rd_length = u16::from_be_bytes([response[offset + 8], response[offset + 9]]) as usize;
        offset += 10;
        if response.len() < offset + rd_length {
            return Err("truncated rdata".to_string());
        }
        if record_type == 1 && rd_length == 4 {
            let ip = Ipv4Addr::new(
                response[offset],
                response[offset + 1],
                response[offset + 2],
                response[offset + 3],
            );
            return Ok(ip);
        }
        offset += rd_length;
    }
    Err("no A record in answers".to_string())
}

/// Пропуск доменного имени с учётом меток и указателя сжатия.
fn skip_name(message: &[u8], mut offset: usize) -> Result<usize, String> {
    loop {
        if offset >= message.len() {
            return Err("name out of bounds".to_string());
        }
        let length = message[offset];
        if length & 0xC0 == 0xC0 {
            return Ok(offset + 2);
        }
        if length == 0 {
            return Ok(offset + 1);
        }
        offset += 1 + length as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_for(query: &[u8], ip: [u8; 4], compress: bool) -> Vec<u8> {
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&[0x81, 0x80]); // QR|RD|RA, rcode 0
        response.extend_from_slice(&[0x00, 0x01]); // 1 question
        response.extend_from_slice(&[0x00, 0x01]); // 1 answer
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        response.extend_from_slice(&query[12..]); // echo вопроса
        if compress {
            response.extend_from_slice(&[0xC0, 0x0C]); // указатель на имя вопроса
        } else {
            response.extend_from_slice(&query[12..query.len() - 4]);
        }
        response.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // A, IN
        response.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // TTL 256
        response.extend_from_slice(&[0x00, 0x04]); // RDLENGTH
        response.extend_from_slice(&ip);
        response
    }

    #[test]
    fn parses_a_record_with_compression() {
        let query = build_query("kws2.fixtelega.co.uk");
        let response = response_for(&query, [104, 21, 64, 155], true);
        assert_eq!(
            parse_a_answer(&query, &response),
            Ok(Ipv4Addr::new(104, 21, 64, 155))
        );
    }

    #[test]
    fn parses_a_record_without_compression() {
        let query = build_query("example.com");
        let response = response_for(&query, [1, 2, 3, 4], false);
        assert_eq!(parse_a_answer(&query, &response), Ok(Ipv4Addr::new(1, 2, 3, 4)));
    }

    #[test]
    fn rejects_mismatched_id_and_rcode() {
        let query = build_query("example.com");
        let mut response = response_for(&query, [1, 2, 3, 4], true);
        response[0] ^= 0xFF;
        assert!(parse_a_answer(&query, &response).is_err());

        let response = response_for(&query, [1, 2, 3, 4], true);
        let mut nx = response.clone();
        nx[3] = (nx[3] & 0xF0) | 0x03; // NXDOMAIN
        assert!(parse_a_answer(&query, &nx).is_err());
    }

    #[test]
    fn skips_cname_answers() {
        let query = build_query("example.com");
        let mut response = response_for(&query, [9, 9, 9, 9], true);
        // заменить тип записи на CNAME (5) с rdata-именем
        let answer_start = response.len() - 16;
        response[answer_start] = 0x00;
        response[answer_start + 1] = 0x05;
        assert!(parse_a_answer(&query, &response).is_err());
    }

    #[test]
    fn query_wire_format() {
        let query = build_query("a.b.example");
        assert_eq!(&query[12..14], b"\x01a");
        assert_eq!(&query[14..16], b"\x01b");
        assert_eq!(&query[16..24], b"\x07example");
        assert_eq!(query[24], 0);
        assert_eq!(&query[25..29], &[0, 1, 0, 1]);
    }
}
