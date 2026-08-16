//! AES-256-CTR с инкрементом всего 128-битного счётчика (big-endian,
//! перенос через все 16 байт) — семантика OpenSSL EVP / Python
//! `cryptography`, которую использует апстрим.

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes256;

pub struct CtrCipher {
    aes: Aes256,
    counter: [u8; 16],
    keystream: [u8; 16],
    pos: usize,
}

impl CtrCipher {
    pub fn new(key: &[u8; 32], iv: &[u8; 16]) -> Self {
        let aes = Aes256::new(key.into());
        let mut counter = [0u8; 16];
        counter.copy_from_slice(iv);
        let mut cipher = Self {
            aes,
            counter,
            keystream: [0u8; 16],
            pos: 16,
        };
        cipher.refill();
        cipher
    }

    fn refill(&mut self) {
        let mut block = aes::Block::from(self.counter);
        self.aes.encrypt_block(&mut block);
        self.keystream = block.into();
        self.pos = 0;
    }

    /// Инкремент 128-битного big-endian счётчика с переносом.
    fn increment(&mut self) {
        for byte in self.counter.iter_mut().rev() {
            let (value, overflow) = byte.overflowing_add(1);
            *byte = value;
            if !overflow {
                break;
            }
        }
    }

    /// Гаммирование буфера на месте. CTR симметричен: одна функция
    /// и для шифрования, и для расшифрования.
    pub fn apply(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            if self.pos == 16 {
                self.increment();
                self.refill();
            }
            *byte ^= self.keystream[self.pos];
            self.pos += 1;
        }
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

    /// SP 800-38A F.5.5: CTR-AES256.Encrypt, первый блок.
    #[test]
    fn sp800_38a_aes256_ctr_first_blocks() {
        let key: [u8; 32] = [
            0x60, 0x3d, 0xeb, 0x10, 0x15, 0xca, 0x71, 0xbe, 0x2b, 0x73, 0xae,
            0xf0, 0x85, 0x7d, 0x77, 0x81, 0x1f, 0x35, 0x2c, 0x07, 0x3b, 0x61,
            0x08, 0xd7, 0x2d, 0x98, 0x10, 0xa3, 0x09, 0x14, 0xdf, 0xf4,
        ];
        let iv: [u8; 16] = [
            0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
            0xfb, 0xfc, 0xfd, 0xfe, 0xff,
        ];
        let plaintext: [u8; 32] = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e,
            0x11, 0x73, 0x93, 0x17, 0x2a, 0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03,
            0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac, 0x45, 0xaf, 0x8e, 0x51,
        ];
        let expected: [u8; 32] = [
            0x60, 0x1e, 0xc3, 0x13, 0x77, 0x57, 0x89, 0xa5, 0xb7, 0xa7, 0xf5,
            0x04, 0xbb, 0xf3, 0xd2, 0x28, 0xf4, 0x43, 0xe3, 0xca, 0x4d, 0x62,
            0xb5, 0x9a, 0xca, 0x84, 0xe9, 0x90, 0xca, 0xca, 0xf5, 0xc5,
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
