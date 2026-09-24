//! Распознавание QUIC Initial по структуре заголовка RFC 9000.
//! ClientHello не расшифровывается. Ответ и таймаут оценивает вызывающий
//! трекер потока; этот модуль только классифицирует пакет UDP/443.

/// Что удалось понять о UDP-пейлоаде на :443.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdpKind {
    /// QUIC long-header Initial (клиентский). Несёт зашифрованный ClientHello.
    QuicInitial,
    /// QUIC short-header (1-RTT) — соединение уже установлено (аналог AppData).
    QuicShort,
    /// UDP-пейлоад, не похожий на QUIC (STUN/DTLS/прочее).
    NonQuic,
    /// Слишком короткий/пустой пейлоад.
    Empty,
}

/// Версии QUIC, которые считаем «боевыми» (v1 = RFC 9000). 0 = Version Negotiation.
const QUIC_V1: u32 = 0x0000_0001;
/// Версия «QUIC v2» (RFC 9369).
const QUIC_V2: u32 = 0x6b33_43cf;
/// Черновики QUIC (0xff0000xx) — всё ещё встречаются.
fn is_draft_version(v: u32) -> bool {
    (v & 0xffff_ff00) == 0xff00_0000
}

/// Тип long-header пакета из первого байта (биты 4-5 у v1). 0 = Initial.
/// Для v2 нумерация типов сдвинута, но Initial-детекции по структуре достаточно.
fn long_packet_type_v1(first: u8) -> u8 {
    (first >> 4) & 0x03
}

/// Классифицирует UDP-пейлоад как QUIC-пакет по СТРУКТУРЕ (без расшифровки).
///
/// Long header (RFC 9000 §17.2): бит 0x80 = long form, бит 0x40 = fixed bit (=1
/// для валидного QUIC). Далее 4 байта версии, затем DCID len + DCID, SCID len + SCID.
/// Short header (§17.3): бит 0x80 = 0, fixed bit 0x40 = 1.
pub fn classify_udp(payload: &[u8]) -> UdpKind {
    if payload.is_empty() {
        return UdpKind::Empty;
    }
    let first = payload[0];
    let fixed_bit = first & 0x40 != 0;
    let long_form = first & 0x80 != 0;

    if long_form {
        // Long header: нужен минимум 1(flags)+4(version)+1(dcid_len).
        if payload.len() < 6 || !fixed_bit {
            return UdpKind::NonQuic;
        }
        let version = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
        // Version Negotiation (version=0) — не Initial.
        if version == 0 {
            return UdpKind::NonQuic;
        }
        let known = version == QUIC_V1 || version == QUIC_V2 || is_draft_version(version);
        if !known {
            return UdpKind::NonQuic;
        }
        // Проверяем правдоподобность DCID: длина в разумных пределах (<=20 по RFC).
        let dcid_len = payload[5] as usize;
        if dcid_len > 20 || payload.len() < 6 + dcid_len {
            return UdpKind::NonQuic;
        }
        // Тип пакета: у v1 Initial = 0. Для v2 тип другой, но это всё равно QUIC
        // long-header — для наших целей (детект «это QUIC-хендшейк») достаточно.
        if version == QUIC_V1 && long_packet_type_v1(first) != 0 {
            // Long-header, но не Initial (Handshake/0-RTT/Retry) — всё ещё QUIC.
            return UdpKind::QuicInitial; // трактуем любой long-header как ранний QUIC
        }
        UdpKind::QuicInitial
    } else if fixed_bit {
        // Short header (1-RTT): соединение уже установлено.
        UdpKind::QuicShort
    } else {
        // Ни long, ни валидный short (fixed bit сброшен) — не QUIC.
        UdpKind::NonQuic
    }
}

/// True, если пейлоад — начало QUIC-хендшейка (клиент инициирует соединение).
pub fn is_quic_handshake_start(payload: &[u8]) -> bool {
    matches!(classify_udp(payload), UdpKind::QuicInitial)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный QUIC v1 long-header Initial с заданной версией/DCID.
    fn quic_long(version: u32, dcid: &[u8], pkt_type: u8) -> Vec<u8> {
        let mut p = Vec::new();
        // 0x80 long form | 0x40 fixed | type<<4.
        p.push(0x80 | 0x40 | ((pkt_type & 0x03) << 4));
        p.extend_from_slice(&version.to_be_bytes());
        p.push(dcid.len() as u8);
        p.extend_from_slice(dcid);
        p.push(0); // scid len = 0
        p.extend_from_slice(&[0u8; 20]); // немного «тела»
        p
    }

    #[test]
    fn detects_quic_v1_initial() {
        let pkt = quic_long(QUIC_V1, &[1, 2, 3, 4, 5, 6, 7, 8], 0);
        assert_eq!(classify_udp(&pkt), UdpKind::QuicInitial);
        assert!(is_quic_handshake_start(&pkt));
    }

    #[test]
    fn detects_quic_v2_long_header() {
        let pkt = quic_long(QUIC_V2, &[9, 9, 9, 9], 1);
        assert_eq!(classify_udp(&pkt), UdpKind::QuicInitial);
    }

    #[test]
    fn detects_draft_version() {
        let pkt = quic_long(0xff00_001d, &[1, 2, 3, 4], 0); // draft-29
        assert_eq!(classify_udp(&pkt), UdpKind::QuicInitial);
    }

    #[test]
    fn version_negotiation_is_not_initial() {
        let pkt = quic_long(0, &[1, 2, 3, 4], 0);
        assert_eq!(classify_udp(&pkt), UdpKind::NonQuic);
    }

    #[test]
    fn unknown_version_rejected() {
        let pkt = quic_long(0xdead_beef, &[1, 2, 3, 4], 0);
        assert_eq!(classify_udp(&pkt), UdpKind::NonQuic);
    }

    #[test]
    fn short_header_is_established_connection() {
        // 0x40 fixed bit, 0x80 сброшен → short header.
        let pkt = vec![0x40, 0xaa, 0xbb, 0xcc];
        assert_eq!(classify_udp(&pkt), UdpKind::QuicShort);
        assert!(!is_quic_handshake_start(&pkt));
    }

    #[test]
    fn stun_and_garbage_are_non_quic() {
        // STUN binding request начинается с 0x00 0x01 — fixed bit сброшен.
        let stun = vec![0x00, 0x01, 0x00, 0x00];
        assert_eq!(classify_udp(&stun), UdpKind::NonQuic);
        // Long form, но fixed bit сброшен (0x80 без 0x40) → не QUIC.
        let bad = vec![0x80, 0x00, 0x00, 0x00, 0x01, 0x04];
        assert_eq!(classify_udp(&bad), UdpKind::NonQuic);
    }

    #[test]
    fn empty_and_short_payloads() {
        assert_eq!(classify_udp(&[]), UdpKind::Empty);
        // Long form заявлен, но слишком короткий для версии+dcid.
        assert_eq!(classify_udp(&[0xc0, 0x00]), UdpKind::NonQuic);
    }

    #[test]
    fn implausible_dcid_len_rejected() {
        let mut pkt = quic_long(QUIC_V1, &[1, 2, 3, 4], 0);
        pkt[5] = 250; // dcid_len > 20 и > доступных байт
        assert_eq!(classify_udp(&pkt), UdpKind::NonQuic);
    }
}
