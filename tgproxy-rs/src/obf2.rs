//! obfuscated2-handshake и производные шифроконтексты (порт
//! `_try_handshake` / `_generate_relay_init` / `_build_crypto_ctx`).
//!
//! Сторона клиента: ключ = SHA256(prekey || secret) — как классический
//! obfuscated2. Сторона релея (к DC по WebSocket, схема webk):
//! ключ = сырой prekey без хеша.

use rand::RngCore;

use crate::crypto::CtrCipher;

pub const HANDSHAKE_LEN: usize = 64;
pub const SKIP_LEN: usize = 8;
pub const PREKEY_LEN: usize = 32;
pub const IV_LEN: usize = 16;
pub const PROTO_TAG_POS: usize = 56;
pub const DC_IDX_POS: usize = 60;

const RESERVED_FIRST_BYTES: [u8; 1] = [0xEF];
const RESERVED_STARTS: [&[u8]; 6] = [
    b"HEAD",
    b"POST",
    b"GET ",
    &[0xee, 0xee, 0xee, 0xee],
    &[0xdd, 0xdd, 0xdd, 0xdd],
    &[0x16, 0x03, 0x01, 0x02],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtoTag {
    Abridged,
    Intermediate,
    Padded,
}

impl ProtoTag {
    pub fn bytes(self) -> [u8; 4] {
        match self {
            ProtoTag::Abridged => [0xef; 4],
            ProtoTag::Intermediate => [0xee; 4],
            ProtoTag::Padded => [0xdd; 4],
        }
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0xef, 0xef, 0xef, 0xef] => Some(ProtoTag::Abridged),
            [0xee, 0xee, 0xee, 0xee] => Some(ProtoTag::Intermediate),
            [0xdd, 0xdd, 0xdd, 0xdd] => Some(ProtoTag::Padded),
            _ => None,
        }
    }
}

/// Разобранный handshake клиента.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInit {
    pub dc: u16,
    pub is_media: bool,
    pub proto_tag: ProtoTag,
}

/// Разбор 64-байтного init клиента: расшифровка хвоста ключом
/// SHA256(prekey || secret) и проверка прото-тега. Неверный secret или
/// чужой протокол → `None` (байт-в-байт как `_try_handshake`).
pub fn parse_client_init(handshake: &[u8; HANDSHAKE_LEN], secret: &[u8; 16]) -> Option<ClientInit> {
    let prekey = &handshake[SKIP_LEN..SKIP_LEN + PREKEY_LEN];
    let iv = &handshake[SKIP_LEN + PREKEY_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN];

    let key = sha256_concat(prekey, secret);
    let mut cipher = CtrCipher::new(&key, iv.try_into().ok()?);
    let mut decrypted = *handshake;
    cipher.apply(&mut decrypted);

    let proto_tag = ProtoTag::from_bytes(&decrypted[PROTO_TAG_POS..PROTO_TAG_POS + 4])?;

    let dc_idx = i16::from_le_bytes([decrypted[DC_IDX_POS], decrypted[DC_IDX_POS + 1]]);

    Some(ClientInit {
        dc: dc_idx.unsigned_abs(),
        is_media: dc_idx < 0,
        proto_tag,
    })
}

fn sha256_concat(a: &[u8], b: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(a);
    hasher.update(b);
    hasher.finalize().into()
}

/// Сборка клиентского init (как его строит клиент Telegram). Нужен как
/// тестам obf2, так и живым пробам против прокси.
#[cfg(test)]
pub(crate) fn build_client_init(
    dc_idx: i16,
    proto_tag: ProtoTag,
    secret: &[u8; 16],
    rng: &mut impl RngCore,
) -> [u8; HANDSHAKE_LEN] {
    let mut init = [0u8; HANDSHAKE_LEN];
    rng.fill_bytes(&mut init);

    let prekey: [u8; 32] = init[SKIP_LEN..SKIP_LEN + PREKEY_LEN].try_into().unwrap();
    let iv: [u8; 16] = init[SKIP_LEN + PREKEY_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN]
        .try_into()
        .unwrap();

    let key = sha256_concat(&prekey, secret);
    let mut cipher = CtrCipher::new(&key, &iv);
    let mut encrypted = init;
    cipher.apply(&mut encrypted);
    let keystream_tail: Vec<u8> = encrypted[56..]
        .iter()
        .zip(init[56..].iter())
        .map(|(a, b)| a ^ b)
        .collect();

    let mut tail = [0u8; 8];
    tail[..4].copy_from_slice(&proto_tag.bytes());
    tail[4..6].copy_from_slice(&dc_idx.to_le_bytes());
    rng.fill_bytes(&mut tail[6..8]);
    for i in 0..8 {
        init[56 + i] = tail[i] ^ keystream_tail[i];
    }
    init
}

/// Генерация 64-байтного relay init для DC (webk-схема: ключ — сырой
/// prekey). Хвост шифруется тем же CTR-потоком, которым позже пойдёт
/// трафик к DC, поэтому его содержимое интегрировано в init.
pub fn make_relay_init(
    proto_tag: ProtoTag,
    dc_idx: i16,
    rng: &mut impl RngCore,
) -> [u8; HANDSHAKE_LEN] {
    loop {
        let mut rnd = [0u8; HANDSHAKE_LEN];
        rng.fill_bytes(&mut rnd);

        if RESERVED_FIRST_BYTES.contains(&rnd[0]) {
            continue;
        }
        if RESERVED_STARTS.iter().any(|start| &rnd[..4] == *start) {
            continue;
        }
        if rnd[4..8] == [0, 0, 0, 0] {
            continue;
        }

        let enc_key: [u8; 32] = rnd[SKIP_LEN..SKIP_LEN + PREKEY_LEN].try_into().unwrap();
        let enc_iv: [u8; 16] = rnd[SKIP_LEN + PREKEY_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN]
            .try_into()
            .unwrap();

        let mut cipher = CtrCipher::new(&enc_key, &enc_iv);
        let mut encrypted_full = rnd;
        cipher.apply(&mut encrypted_full);

        let keystream_tail: [u8; 8] = std::iter::zip(&encrypted_full[56..], &rnd[56..])
            .map(|(a, b)| a ^ b)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let mut tail_plain = [0u8; 8];
        tail_plain[..4].copy_from_slice(&proto_tag.bytes());
        tail_plain[4..6].copy_from_slice(&dc_idx.to_le_bytes());
        rng.fill_bytes(&mut tail_plain[6..8]);

        for (i, byte) in tail_plain.iter().enumerate() {
            rnd[56 + i] = byte ^ keystream_tail[i];
        }
        return rnd;
    }
}

/// Четыре AES-CTR-потока одного соединения (порт CryptoCtx):
/// clt_dec — расшифровка от клиента, clt_enc — шифрование к клиенту,
/// tg_enc — шифрование к DC, tg_dec — расшифровка от DC.
pub struct CryptoCtx {
    pub clt_dec: CtrCipher,
    pub clt_enc: CtrCipher,
    pub tg_enc: CtrCipher,
    pub tg_dec: CtrCipher,
}

impl CryptoCtx {
    /// `client_dec_prekey_iv` — 48 байт init клиента `[8..56]`,
    /// `relay_init` — сгенерированный нами init к DC.
    pub fn build(
        client_dec_prekey_iv: &[u8],
        secret: &[u8; 16],
        relay_init: &[u8; HANDSHAKE_LEN],
    ) -> Self {
        // Клиентская сторона: ключи с хешем секрета.
        let clt_dec_key = sha256_concat(&client_dec_prekey_iv[..PREKEY_LEN], secret);
        let clt_dec_iv: [u8; 16] = client_dec_prekey_iv[PREKEY_LEN..PREKEY_LEN + IV_LEN]
            .try_into()
            .unwrap();
        let mut clt_dec = CtrCipher::new(&clt_dec_key, &clt_dec_iv);
        clt_dec.skip_64();

        let reversed: Vec<u8> = client_dec_prekey_iv.iter().rev().copied().collect();
        let clt_enc_key = sha256_concat(&reversed[..PREKEY_LEN], secret);
        let clt_enc_iv: [u8; 16] = reversed[PREKEY_LEN..PREKEY_LEN + IV_LEN]
            .try_into()
            .unwrap();
        let clt_enc = CtrCipher::new(&clt_enc_key, &clt_enc_iv);

        // Релейная сторона (webk): сырые ключи без секрета.
        let relay_enc_key: [u8; 32] = relay_init[SKIP_LEN..SKIP_LEN + PREKEY_LEN]
            .try_into()
            .unwrap();
        let relay_enc_iv: [u8; 16] = relay_init
            [SKIP_LEN + PREKEY_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN]
            .try_into()
            .unwrap();
        let mut tg_enc = CtrCipher::new(&relay_enc_key, &relay_enc_iv);
        tg_enc.skip_64();

        let relay_reversed: Vec<u8> = relay_init[SKIP_LEN..SKIP_LEN + PREKEY_LEN + IV_LEN]
            .iter()
            .rev()
            .copied()
            .collect();
        let tg_dec_key: [u8; 32] = relay_reversed[..PREKEY_LEN].try_into().unwrap();
        let tg_dec_iv: [u8; 16] = relay_reversed[PREKEY_LEN..PREKEY_LEN + IV_LEN]
            .try_into()
            .unwrap();
        let tg_dec = CtrCipher::new(&tg_dec_key, &tg_dec_iv);

        CryptoCtx {
            clt_dec,
            clt_enc,
            tg_enc,
            tg_dec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    const SECRET: [u8; 16] = [0x11; 16];

    #[test]
    fn parses_regular_and_media_client_init() {
        let init = build_client_init(2, ProtoTag::Abridged, &SECRET, &mut OsRng);
        assert_eq!(
            parse_client_init(&init, &SECRET),
            Some(ClientInit {
                dc: 2,
                is_media: false,
                proto_tag: ProtoTag::Abridged
            })
        );

        let init = build_client_init(-2, ProtoTag::Intermediate, &SECRET, &mut OsRng);
        assert_eq!(
            parse_client_init(&init, &SECRET),
            Some(ClientInit {
                dc: 2,
                is_media: true,
                proto_tag: ProtoTag::Intermediate
            })
        );
    }

    #[test]
    fn test_dc_ids_pass_through() {
        // dc >= 10000 — тестовый DC, разбирается как обычное число.
        let init = build_client_init(10_002, ProtoTag::Padded, &SECRET, &mut OsRng);
        assert_eq!(
            parse_client_init(&init, &SECRET).map(|c| c.dc),
            Some(10_002)
        );
    }

    #[test]
    fn wrong_secret_rejected() {
        let init = build_client_init(2, ProtoTag::Abridged, &SECRET, &mut OsRng);
        let other_secret = [0x22; 16];
        assert_eq!(parse_client_init(&init, &other_secret), None);
    }

    #[test]
    fn relay_init_roundtrip() {
        let relay = make_relay_init(ProtoTag::Abridged, -4, &mut OsRng);

        // DC восстанавливает тег и dc из хвоста сырым ключом из init.
        let key: [u8; 32] = relay[8..40].try_into().unwrap();
        let iv: [u8; 16] = relay[40..56].try_into().unwrap();
        let mut cipher = CtrCipher::new(&key, &iv);
        let mut decrypted = relay;
        cipher.apply(&mut decrypted);

        assert_eq!(&decrypted[56..60], &[0xef; 4]);
        assert_eq!(i16::from_le_bytes([decrypted[60], decrypted[61]]), -4);
    }

    #[test]
    fn relay_init_avoids_reserved_prefixes() {
        for _ in 0..200 {
            let relay = make_relay_init(ProtoTag::Intermediate, 2, &mut OsRng);
            assert_ne!(relay[0], 0xEF);
            assert!(!RESERVED_STARTS.iter().any(|s| &relay[..4] == *s));
            assert_ne!(relay[4..8], [0, 0, 0, 0]);
        }
    }

    #[test]
    fn crypto_ctx_roundtrips_both_directions() {
        let client_init = build_client_init(2, ProtoTag::Abridged, &SECRET, &mut OsRng);
        let relay = make_relay_init(ProtoTag::Abridged, 2, &mut OsRng);
        let mut ctx = CryptoCtx::build(&client_init[8..56], &SECRET, &relay);

        // Uplink: клиент шифрует зеркалом clt_dec; мост расшифровывает
        // clt_dec и перешифровывает tg_enc; DC расшифровывает зеркалом
        // tg_enc (сырой ключ, skip_64).
        let mut payload = [0x5au8; 100];
        OsRng.fill_bytes(&mut payload);

        let client_key = sha256_concat(&client_init[8..40], &SECRET);
        let client_iv: [u8; 16] = client_init[40..56].try_into().unwrap();
        let mut client_enc = CtrCipher::new(&client_key, &client_iv);
        client_enc.skip_64();
        let mut uplink = payload;
        client_enc.apply(&mut uplink);

        let mut decrypted = uplink;
        ctx.clt_dec.apply(&mut decrypted);
        assert_eq!(decrypted, payload);

        let mut to_dc = payload;
        ctx.tg_enc.apply(&mut to_dc);

        let dc_key: [u8; 32] = relay[8..40].try_into().unwrap();
        let dc_iv: [u8; 16] = relay[40..56].try_into().unwrap();
        let mut dc_dec = CtrCipher::new(&dc_key, &dc_iv);
        dc_dec.skip_64();
        let mut dc_plain = to_dc;
        dc_dec.apply(&mut dc_plain);
        assert_eq!(dc_plain, payload);

        // Downlink: DC шифрует ключом из перевёрнутого нашего init;
        // мост расшифровывает tg_dec и шифрует к клиенту clt_enc;
        // клиент снимает clt_enc зеркалом (перевёрнутый init + секрет).
        let relay_reversed: Vec<u8> = relay[8..56].iter().rev().copied().collect();
        let dc_enc_key: [u8; 32] = relay_reversed[..32].try_into().unwrap();
        let dc_enc_iv: [u8; 16] = relay_reversed[32..48].try_into().unwrap();
        let mut dc_enc = CtrCipher::new(&dc_enc_key, &dc_enc_iv);
        let mut downlink = payload;
        dc_enc.apply(&mut downlink);

        let mut down_plain = downlink;
        ctx.tg_dec.apply(&mut down_plain);
        assert_eq!(down_plain, payload);

        let mut to_client = down_plain;
        ctx.clt_enc.apply(&mut to_client);

        let client_reversed: Vec<u8> = client_init[8..56].iter().rev().copied().collect();
        let client_dec_key = sha256_concat(&client_reversed[..32], &SECRET);
        let client_dec_iv: [u8; 16] = client_reversed[32..48].try_into().unwrap();
        let mut client_dec = CtrCipher::new(&client_dec_key, &client_dec_iv);
        let mut final_plain = to_client;
        client_dec.apply(&mut final_plain);
        assert_eq!(final_plain, payload);
    }
}
