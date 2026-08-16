//! Разбиение потока на MTProto-транспортные пакеты (порт MsgSplitter),
//! чтобы каждый пакет уезжал к DC отдельным WS-фреймом.
//!
//! Отличия от апстрима: сплиттеру передаётся уже расшифрованный поток
//! (вывод clt_dec), поэтому зеркальный третий AES-контекст не нужен.
//! Расхождение намеренное: заявленная длина пакета ограничена 1 МиБ —
//! выше реальных MTProto-пакетов и ниже лимита WS-сообщения CF (1 МиБ),
//! вместо неограниченного буфера до 2 ГиБ.

const MAX_PACKET_LEN: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Abridged,
    Intermediate,
    Padded,
}

pub struct MsgSplitter {
    proto: Proto,
    cipher_buf: Vec<u8>,
    plain_buf: Vec<u8>,
    disabled: bool,
}

impl MsgSplitter {
    pub fn new(proto: Proto) -> Self {
        MsgSplitter {
            proto,
            cipher_buf: Vec::new(),
            plain_buf: Vec::new(),
            disabled: false,
        }
    }

    /// Принимает очередной кусок потока: `plain` — расшифрованные байты
    /// (для разбора длин), `cipher` — те же байты в шифрованном виде к DC
    /// (для выдачи наружу). Оба куска обязаны быть одинаковой длины и
    /// продолжать поток с места, где закончился предыдущий.
    pub fn split(&mut self, plain: &[u8], cipher: &[u8]) -> Vec<Vec<u8>> {
        debug_assert_eq!(plain.len(), cipher.len());
        if plain.is_empty() {
            return Vec::new();
        }
        if self.disabled {
            return vec![cipher.to_vec()];
        }

        self.plain_buf.extend_from_slice(plain);
        self.cipher_buf.extend_from_slice(cipher);

        let mut parts = Vec::new();
        let mut offset = 0usize;
        let buf_len = self.cipher_buf.len();
        while offset < buf_len {
            match self.next_packet_len(offset) {
                // Пакет ещё не помещается в буфер целиком — ждём данных.
                None => break,
                // Некорректная длина — дальнейший разбор невозможен,
                // переводим сплиттер в режим прозрачной передачи.
                Some(0) => {
                    parts.push(self.cipher_buf[offset..].to_vec());
                    offset = buf_len;
                    self.disabled = true;
                    break;
                }
                Some(len) => {
                    parts.push(self.cipher_buf[offset..offset + len].to_vec());
                    offset += len;
                }
            }
        }

        if offset > 0 {
            self.plain_buf.drain(..offset);
            self.cipher_buf.drain(..offset);
        }
        parts
    }

    /// Хвост, не собранный в пакет к моменту закрытия клиентского потока.
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.cipher_buf.is_empty() {
            return None;
        }
        let tail = std::mem::take(&mut self.cipher_buf);
        self.plain_buf.clear();
        Some(tail)
    }

    fn next_packet_len(&self, offset: usize) -> Option<usize> {
        let avail = self.plain_buf.len() - offset;
        if avail == 0 {
            return None;
        }
        match self.proto {
            Proto::Abridged => self.next_abridged_len(offset, avail),
            Proto::Intermediate | Proto::Padded => self.next_intermediate_len(offset, avail),
        }
    }

    fn next_abridged_len(&self, offset: usize, avail: usize) -> Option<usize> {
        let first = self.plain_buf[offset];
        let (payload_len, header_len) = if first == 0x7f || first == 0xff {
            if avail < 4 {
                return None;
            }
            let length = u32::from_le_bytes([
                self.plain_buf[offset + 1],
                self.plain_buf[offset + 2],
                self.plain_buf[offset + 3],
                0,
            ]);
            (length as usize * 4, 4)
        } else {
            ((first & 0x7f) as usize * 4, 1)
        };
        if payload_len == 0 || payload_len > MAX_PACKET_LEN {
            return Some(0);
        }
        let packet_len = header_len + payload_len;
        if packet_len > MAX_PACKET_LEN {
            return Some(0);
        }
        if avail < packet_len {
            return None;
        }
        Some(packet_len)
    }

    fn next_intermediate_len(&self, offset: usize, avail: usize) -> Option<usize> {
        if avail < 4 {
            return None;
        }
        let payload_len = u32::from_le_bytes(self.plain_buf[offset..offset + 4].try_into().unwrap())
            as usize
            & 0x7fff_ffff;
        if payload_len == 0 || payload_len > MAX_PACKET_LEN {
            return Some(0);
        }
        let packet_len = 4 + payload_len;
        if avail < packet_len {
            return None;
        }
        Some(packet_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(proto: Proto, plain: &[&[u8]], cipher_marker: u8) -> Vec<Vec<u8>> {
        let mut splitter = MsgSplitter::new(proto);
        let mut out = Vec::new();
        for chunk in plain {
            // Шифрованный вид эмулируем инкрементом байта — важно только
            // соответствие длин и позиций.
            let cipher: Vec<u8> = chunk
                .iter()
                .map(|b| b.wrapping_add(cipher_marker))
                .collect();
            out.extend(splitter.split(chunk, &cipher));
        }
        out
    }

    #[test]
    fn abridged_single_short_packet() {
        // header 0x02 -> payload 8 байт, пакет целиком 9.
        let packet: &[u8] = &[&[0x02][..], &[0xaa; 8][..]].concat();
        let parts = run(Proto::Abridged, &[packet], 1);
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0],
            packet.iter().map(|b| b.wrapping_add(1)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn abridged_two_packets_in_one_chunk() {
        let p1: Vec<u8> = [&[0x01u8][..], &[0x01; 4][..]].concat();
        let p2: Vec<u8> = [&[0x03u8][..], &[0x02; 12][..]].concat();
        let mut stream = p1.clone();
        stream.extend_from_slice(&p2);
        let parts = run(Proto::Abridged, &[&stream], 0);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], p1);
        assert_eq!(parts[1], p2);
    }

    #[test]
    fn abridged_long_form_uses_24bit_length() {
        // 0x7f + u24 LE = 3 -> payload 12 байт, header 4.
        let packet: Vec<u8> = [&[0x7fu8, 0x03, 0x00, 0x00][..], &[0x99; 12][..]].concat();
        let parts = run(Proto::Abridged, &[&packet], 0);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], packet);
    }

    #[test]
    fn packet_split_across_chunks() {
        let packet: Vec<u8> = [&[0x02u8][..], &[0xbb; 8][..]].concat();
        let parts = run(Proto::Abridged, &[&packet[..4], &packet[4..]], 0);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], packet);
    }

    #[test]
    fn intermediate_and_padded_share_length_logic() {
        let packet: Vec<u8> = [&(9u32).to_le_bytes()[..], &[0xcc; 9][..]].concat();
        for proto in [Proto::Intermediate, Proto::Padded] {
            let parts = run(proto, &[&packet], 0);
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0], packet);
        }
    }

    #[test]
    fn invalid_length_disables_splitter() {
        // intermediate с длиной 0 — некорректно: хвост уходит as-is,
        // дальнейшие чанки проходят насквозь без разбора.
        let mut splitter = MsgSplitter::new(Proto::Intermediate);
        let bogus: Vec<u8> = 0u32.to_le_bytes().to_vec();
        let cipher = bogus.clone();
        let parts = splitter.split(&bogus, &cipher);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], bogus);

        let later = vec![0xde, 0xad, 0xbe, 0xef];
        let parts = splitter.split(&later.clone(), &later.clone());
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], later);
    }

    #[test]
    fn over_cap_length_disables_instead_of_buffering() {
        let mut splitter = MsgSplitter::new(Proto::Intermediate);
        let header = (MAX_PACKET_LEN as u32 + 1).to_le_bytes();
        let mut bogus = header.to_vec();
        bogus.extend_from_slice(&[0x00; 8]);
        let cipher = bogus.clone();
        let parts = splitter.split(&bogus, &cipher);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], bogus);
    }

    #[test]
    fn flush_returns_leftover_tail() {
        let mut splitter = MsgSplitter::new(Proto::Abridged);
        let partial = [0x02u8, 0xaa, 0xaa];
        let cipher = partial.to_vec();
        assert!(splitter.split(&partial, &cipher).is_empty());
        let tail = splitter.flush().expect("tail must be flushed");
        assert_eq!(tail, partial.to_vec());
        assert!(splitter.flush().is_none());
    }
}
