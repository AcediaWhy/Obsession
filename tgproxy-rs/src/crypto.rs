//! AES-256-CTR с полным 128-битным big-endian счётчиком — семантика
//! OpenSSL EVP / Python `cryptography`, которую использует апстрим.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes256;

type Aes256Ctr = ctr::Ctr128BE<Aes256>;

pub struct CtrCipher(Aes256Ctr);

impl CtrCipher {
    pub fn new(key: &[u8; 32], iv: &[u8; 16]) -> Self {
        Self(Aes256Ctr::new(key.into(), iv.into()))
    }

    /// Гаммирование буфера на месте. CTR симметричен: одна функция
    /// и для шифрования, и для расшифрования.
    pub fn apply(&mut self, data: &mut [u8]) {
        self.0.apply_keystream(data);
    }

    /// Пропуск 64 байт гаммы — позиционирование после 64-байтного init,
    /// который занимал первые четыре блока потока (`update(ZERO_64)`
    /// в апстриме).
    pub fn skip_64(&mut self) {
        let mut sink = [0u8; 64];
        self.apply(&mut sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncrypt, KeyInit};

    /// SP 800-38A F.5.5: полный четырёхблочный CTR-AES256.Encrypt.
    #[test]
    fn sp800_38a_aes256_ctr_first_blocks() {
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae, 0xf0, 0x85, 0x7d,
            0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61, 0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3,
            0x09, 0x14, 0xdf, 0xf4,
        ];
        let iv: [u8; 16] = [
            0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd,
            0xfe, 0xff,
        ];
        let plaintext: [u8; 64] = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a, 0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03, 0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac,
            0x45, 0xaf, 0x8e, 0x51, 0x30, 0xc8, 0x1c, 0x46, 0xa3, 0x5c, 0xe4, 0x11, 0xe5, 0xfb,
            0xc1, 0x19, 0x1a, 0x0a, 0x52, 0xef, 0xf6, 0x9f, 0x24, 0x45, 0xdf, 0x4f, 0x9b, 0x17,
            0xad, 0x2b, 0x41, 0x7b, 0xe6, 0x6c, 0x37, 0x10,
        ];
        let expected: [u8; 64] = [
            0x60, 0x1e, 0xc3, 0x13, 0x77, 0x57, 0x89, 0xa5, 0xb7, 0xa7, 0xf5, 0x04, 0xbb, 0xf3,
            0xd2, 0x28, 0xf4, 0x43, 0xe3, 0xca, 0x4d, 0x62, 0xb5, 0x9a, 0xca, 0x84, 0xe9, 0x90,
            0xca, 0xca, 0xf5, 0xc5, 0x2b, 0x09, 0x30, 0xda, 0xa2, 0x3d, 0xe9, 0x4c, 0xe8, 0x70,
            0x17, 0xba, 0x2d, 0x84, 0x98, 0x8d, 0xdf, 0xc9, 0xc5, 0x8d, 0xb6, 0x7a, 0xad, 0xa6,
            0x13, 0xc2, 0xdd, 0x08, 0x45, 0x79, 0x41, 0xa6,
        ];

        let mut cipher = CtrCipher::new(&key, &iv);
        let mut data = plaintext;
        cipher.apply(&mut data);
        assert_eq!(data, expected);

        // Симметричность: свежий шифр с теми же key/iv расшифровывает
        // поток, продолжающийся вторым блоком.
        let mut decryptor = CtrCipher::new(&key, &iv);
        let mut again = data;
        decryptor.apply(&mut again);
        assert_eq!(again, plaintext);
    }

    #[test]
    fn counter_wraps_across_full_block() {
        // IV = 0xff..ff -> после первого блока счётчик переносится во все
        // 16 байт и становится 0x00..00.
        let key = [7u8; 32];
        let mut cipher = CtrCipher::new(&key, &[0xff; 16]);
        let mut first = [0u8; 16];
        cipher.apply(&mut first);
        let mut second = [0u8; 16];
        cipher.apply(&mut second);

        let reference_first = {
            let aes = Aes256::new((&key).into());
            let mut block = aes::Block::from([0xff; 16]);
            aes.encrypt_block(&mut block);
            let block: [u8; 16] = block.into();
            block
        };
        assert_eq!(first, reference_first);

        let reference_second = {
            let aes = Aes256::new((&key).into());
            let mut block = aes::Block::from([0x00; 16]);
            aes.encrypt_block(&mut block);
            let block: [u8; 16] = block.into();
            block
        };
        assert_eq!(second, reference_second);
    }

    #[test]
    fn chunked_apply_matches_one_shot_across_unaligned_boundaries() {
        let key = [0x5au8; 32];
        let iv = [0xa5u8; 16];
        let input: Vec<u8> = (0..4097)
            .map(|offset| (offset as u8).wrapping_mul(29).wrapping_add(7))
            .collect();

        let mut expected = input.clone();
        CtrCipher::new(&key, &iv).apply(&mut expected);

        let mut actual = input;
        let mut chunked = CtrCipher::new(&key, &iv);
        let chunk_sizes = [1usize, 15, 17, 31, 64, 257, 1024];
        let mut offset = 0;
        let mut chunk_index = 0;
        while offset < actual.len() {
            chunked.apply(&mut []);
            let end = (offset + chunk_sizes[chunk_index % chunk_sizes.len()]).min(actual.len());
            chunked.apply(&mut actual[offset..end]);
            offset = end;
            chunk_index += 1;
        }

        assert_eq!(actual, expected);
    }

    #[test]
    fn skip_64_advances_four_blocks() {
        let key = [9u8; 32];
        let iv = [1u8; 16];
        let mut skipped = CtrCipher::new(&key, &iv);
        skipped.skip_64();

        let mut fresh = CtrCipher::new(&key, &iv);
        let mut sink = [0u8; 64];
        fresh.apply(&mut sink);

        let mut probe_skipped = [0x42u8; 8];
        skipped.apply(&mut probe_skipped);
        let mut probe_fresh = [0x42u8; 8];
        fresh.apply(&mut probe_fresh);
        assert_eq!(probe_skipped, probe_fresh);
    }
}
