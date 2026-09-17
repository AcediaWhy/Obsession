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
    plain_header: [u8; 4],
    cipher_header: [u8; 4],
    header_len: usize,
    packet_len: Option<usize>,
    disabled: bool,
}

impl MsgSplitter {
    pub fn new(proto: Proto) -> Self {
        MsgSplitter {
            proto,
            cipher_buf: Vec::new(),
            plain_header: [0; 4],
            cipher_header: [0; 4],
            header_len: 0,
            packet_len: None,
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

        let mut parts = Vec::new();
        let mut offset = 0usize;
        while offset < plain.len() {
            if self.packet_len.is_none() {
                let header_len = self.required_header_len();
                let take = (header_len - self.header_len).min(plain.len() - offset);
                let end = offset + take;
                self.plain_header[self.header_len..self.header_len + take]
                    .copy_from_slice(&plain[offset..end]);
                self.cipher_header[self.header_len..self.header_len + take]
                    .copy_from_slice(&cipher[offset..end]);
                self.header_len += take;
                offset = end;

                // У abridged первый байт определяет, нужен ли длинный
                // четырёхбайтовый заголовок. Он может приехать отдельно.
                if self.header_len < self.required_header_len() {
                    continue;
                }

                let packet_len = self.packet_len_from_header();
                if packet_len == 0 {
                    // Сохраняем прежнюю fail-open семантику: накопленный
                    // некорректный пакет и остаток текущего чанка уходят
                    // одним сообщением, следующие чанки — напрямую.
                    let mut tail = Vec::with_capacity(self.header_len + cipher.len() - offset);
                    tail.extend_from_slice(&self.cipher_header[..self.header_len]);
                    tail.extend_from_slice(&cipher[offset..]);
                    self.reset_current_packet();
                    parts.push(tail);
                    self.disabled = true;
                    return parts;
                }
                self.packet_len = Some(packet_len);
                if offset == plain.len() {
                    continue;
                }
            }

            let packet_len = self.packet_len.expect("packet length was parsed");
            if self.cipher_buf.is_empty() {
                let available_payload = (packet_len - self.header_len).min(plain.len() - offset);
                self.cipher_buf = Vec::with_capacity(self.header_len + available_payload);
                self.cipher_buf
                    .extend_from_slice(&self.cipher_header[..self.header_len]);
            }
            let take = (packet_len - self.cipher_buf.len()).min(plain.len() - offset);
            let end = offset + take;
            self.cipher_buf.extend_from_slice(&cipher[offset..end]);
            offset = end;

            if self.cipher_buf.len() == packet_len {
                // Передаём наружу сам накопительный Vec. В отличие от
                // прежнего to_vec(), это не копирует готовый пакет и не
                // оставляет мегабайтную capacity жить до конца сессии.
                parts.push(self.take_current_packet());
            }
        }
        parts
    }

    /// Хвост, не собранный в пакет к моменту закрытия клиентского потока.
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.cipher_buf.is_empty() && self.header_len == 0 {
            return None;
        }
        Some(self.take_current_packet())
    }

    fn required_header_len(&self) -> usize {
        match self.proto {
            Proto::Abridged
                if self.header_len > 0
                    && (self.plain_header[0] == 0x7f || self.plain_header[0] == 0xff) =>
            {
                4
            }
            Proto::Abridged => 1,
            Proto::Intermediate | Proto::Padded => 4,
        }
    }

    fn packet_len_from_header(&self) -> usize {
        debug_assert_eq!(self.header_len, self.required_header_len());
        match self.proto {
            Proto::Abridged => self.abridged_packet_len(),
            Proto::Intermediate | Proto::Padded => self.intermediate_packet_len(),
        }
    }

    fn abridged_packet_len(&self) -> usize {
        let first = self.plain_header[0];
        let (payload_len, header_len) = if first == 0x7f || first == 0xff {
            let length = u32::from_le_bytes([
                self.plain_header[1],
                self.plain_header[2],
                self.plain_header[3],
                0,
            ]);
            (length as usize * 4, 4)
        } else {
            ((first & 0x7f) as usize * 4, 1)
        };
        if payload_len == 0 || payload_len > MAX_PACKET_LEN {
            return 0;
        }
        let packet_len = header_len + payload_len;
        if packet_len > MAX_PACKET_LEN {
            return 0;
        }
        packet_len
    }

    fn intermediate_packet_len(&self) -> usize {
        let payload_len = u32::from_le_bytes(self.plain_header) as usize & 0x7fff_ffff;
        if payload_len == 0 || payload_len > MAX_PACKET_LEN {
            return 0;
        }
        4 + payload_len
    }

    fn take_current_packet(&mut self) -> Vec<u8> {
        let packet = if self.cipher_buf.is_empty() {
            self.cipher_header[..self.header_len].to_vec()
        } else {
            std::mem::take(&mut self.cipher_buf)
        };
        self.reset_current_packet();
        packet
    }

    fn reset_current_packet(&mut self) {
        self.header_len = 0;
        self.packet_len = None;
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

    fn run_paired(proto: Proto, plain: &[&[u8]], cipher: &[&[u8]]) -> Vec<Vec<u8>> {
        assert_eq!(plain.len(), cipher.len());
        let mut splitter = MsgSplitter::new(proto);
        let mut out = Vec::new();
        for (plain_chunk, cipher_chunk) in plain.iter().zip(cipher) {
            out.extend(splitter.split(plain_chunk, cipher_chunk));
        }
        out
    }

    fn positional_cipher(stream: &[u8]) -> Vec<u8> {
        stream
            .iter()
            .enumerate()
            .map(|(offset, byte)| byte ^ (offset as u8).wrapping_mul(73).wrapping_add(0xa5))
            .collect()
    }

    fn assert_all_three_chunk_partitions(proto: Proto, packets: &[Vec<u8>]) {
        let stream: Vec<u8> = packets.concat();
        let cipher = positional_cipher(&stream);
        let mut expected_offset = 0;
        let expected: Vec<Vec<u8>> = packets
            .iter()
            .map(|packet| {
                let end = expected_offset + packet.len();
                let part = cipher[expected_offset..end].to_vec();
                expected_offset = end;
                part
            })
            .collect();

        for first in 0..=stream.len() {
            for second in first..=stream.len() {
                let plain_chunks = [&stream[..first], &stream[first..second], &stream[second..]];
                let cipher_chunks = [&cipher[..first], &cipher[first..second], &cipher[second..]];
                assert_eq!(
                    run_paired(proto, &plain_chunks, &cipher_chunks),
                    expected,
                    "failed for {proto:?} at cuts {first}/{second}"
                );
            }
        }

        let plain_byte_chunks: Vec<&[u8]> = stream.chunks(1).collect();
        let cipher_byte_chunks: Vec<&[u8]> = cipher.chunks(1).collect();
        assert_eq!(
            run_paired(proto, &plain_byte_chunks, &cipher_byte_chunks),
            expected
        );
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
        // 0x7f/0xff + u24 LE = 3 -> payload 12 байт, header 4.
        for marker in [0x7fu8, 0xff] {
            let packet: Vec<u8> = [&[marker, 0x03, 0x00, 0x00][..], &[0x99; 12][..]].concat();
            let parts = run(Proto::Abridged, &[&packet], 0);
            assert_eq!(parts, vec![packet]);
        }
    }

    #[test]
    fn abridged_quick_ack_short_form_masks_the_high_bit() {
        let packet: Vec<u8> = [&[0x81u8][..], &[0x88; 4][..]].concat();
        assert_eq!(run(Proto::Abridged, &[&packet], 0), vec![packet]);
    }

    #[test]
    fn packet_split_across_chunks() {
        let packet: Vec<u8> = [&[0x02u8][..], &[0xbb; 8][..]].concat();
        let parts = run(Proto::Abridged, &[&packet[..4], &packet[4..]], 0);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], packet);
    }

    #[test]
    fn header_only_uses_inline_storage_until_payload_arrives() {
        let mut splitter = MsgSplitter::new(Proto::Intermediate);
        let header = 64u32.to_le_bytes();
        assert!(splitter.split(&header, &header).is_empty());
        assert_eq!(splitter.cipher_buf.capacity(), 0);
        assert_eq!(splitter.flush(), Some(header.to_vec()));
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
    fn packet_output_is_invariant_across_header_and_body_chunk_boundaries() {
        let abridged = vec![
            [&[0x01u8][..], &[0x11; 4][..]].concat(),
            [&[0x7fu8, 0x03, 0x00, 0x00][..], &[0x22; 12][..]].concat(),
            [&[0x02u8][..], &[0x33; 8][..]].concat(),
        ];
        assert_all_three_chunk_partitions(Proto::Abridged, &abridged);

        let intermediate = vec![
            [&5u32.to_le_bytes()[..], &[0x44; 5][..]].concat(),
            [&9u32.to_le_bytes()[..], &[0x55; 9][..]].concat(),
        ];
        assert_all_three_chunk_partitions(Proto::Intermediate, &intermediate);

        let padded = vec![
            [&0x8000_0005u32.to_le_bytes()[..], &[0x66; 5][..]].concat(),
            [&7u32.to_le_bytes()[..], &[0x77; 7][..]].concat(),
        ];
        assert_all_three_chunk_partitions(Proto::Padded, &padded);
    }

    #[test]
    fn every_chunk_partition_of_a_compact_stream_is_equivalent() {
        let packets = [
            [&[0x01u8][..], &[0x10; 4][..]].concat(),
            [&[0x01u8][..], &[0x20; 4][..]].concat(),
            [&[0x01u8][..], &[0x30; 4][..]].concat(),
        ];
        let stream: Vec<u8> = packets.concat();
        let cipher = positional_cipher(&stream);
        let expected = vec![
            cipher[..5].to_vec(),
            cipher[5..10].to_vec(),
            cipher[10..].to_vec(),
        ];
        let boundary_count = stream.len() - 1;

        for mask in 0usize..(1usize << boundary_count) {
            let mut plain_chunks = Vec::new();
            let mut cipher_chunks = Vec::new();
            let mut start = 0;
            for boundary in 0..boundary_count {
                if mask & (1usize << boundary) != 0 {
                    plain_chunks.push(&stream[start..=boundary]);
                    cipher_chunks.push(&cipher[start..=boundary]);
                    start = boundary + 1;
                }
            }
            plain_chunks.push(&stream[start..]);
            cipher_chunks.push(&cipher[start..]);

            assert_eq!(
                run_paired(Proto::Abridged, &plain_chunks, &cipher_chunks),
                expected
            );
        }
    }

    #[test]
    fn invalid_packet_after_valid_packet_preserves_fail_open_boundaries() {
        let valid: Vec<u8> = [&4u32.to_le_bytes()[..], &[0xaa; 4][..]].concat();
        let invalid_and_suffix: Vec<u8> = [&0u32.to_le_bytes()[..], &[0xde, 0xad][..]].concat();
        let mut stream = valid.clone();
        stream.extend_from_slice(&invalid_and_suffix);

        let mut splitter = MsgSplitter::new(Proto::Intermediate);
        let parts = splitter.split(&stream, &stream);
        assert_eq!(parts, vec![valid, invalid_and_suffix]);

        let later = [0xbe, 0xef];
        assert_eq!(splitter.split(&later, &later), vec![later.to_vec()]);
    }

    #[test]
    fn split_invalid_header_flushes_the_whole_cipher_tail() {
        let invalid = [0x00, 0x00, 0x00, 0x00, 0xde, 0xad];
        for cut in 1..4 {
            let mut splitter = MsgSplitter::new(Proto::Intermediate);
            assert!(splitter.split(&invalid[..cut], &invalid[..cut]).is_empty());
            assert_eq!(
                splitter.split(&invalid[cut..], &invalid[cut..]),
                vec![invalid.to_vec()]
            );
        }
    }

    #[test]
    fn completed_large_packet_leaves_no_retained_payload_capacity() {
        let payload_len = 256 * 1024;
        let words = (payload_len / 4) as u32;
        let encoded_words = words.to_le_bytes();
        let mut packet = vec![0x7f, encoded_words[0], encoded_words[1], encoded_words[2]];
        packet.resize(packet.len() + payload_len, 0xab);

        let mut splitter = MsgSplitter::new(Proto::Abridged);
        let mut parts = Vec::new();
        for chunk in packet.chunks(8191) {
            parts.extend(splitter.split(chunk, chunk));
        }

        assert_eq!(parts, vec![packet]);
        assert_eq!(splitter.cipher_buf.capacity(), 0);
        assert_eq!(splitter.header_len, 0);
        assert_eq!(splitter.packet_len, None);
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

        let complete: Vec<u8> = [&[0x01u8][..], &[0xbb; 4][..]].concat();
        assert_eq!(splitter.split(&complete, &complete), vec![complete]);
    }
}
