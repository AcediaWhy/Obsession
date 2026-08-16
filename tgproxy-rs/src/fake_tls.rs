//! FakeTLS-маскировка (порт fake_tls.py апстрима, схема mtg): клиент
//! оборачивает MTProto-сессию в TLS-подобные записи, аутентифицируясь
//! HMAC-ом с секретом прокси; соединения без правильной аутентификации
//! прозрачно пробрасываются на домен маскировки.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::logger;

pub const TLS_RECORD_HANDSHAKE: u8 = 0x16;
const TLS_RECORD_CCS: u8 = 0x14;
const TLS_RECORD_APPDATA: u8 = 0x17;

const CLIENT_RANDOM_OFFSET: usize = 11;
const SESSION_ID_OFFSET: usize = 44;
const TIMESTAMP_TOLERANCE: i64 = 120;
const TLS_APPDATA_MAX: usize = 16384;

type HmacSha256 = Hmac<Sha256>;

/// Шаблон ServerHello (порт _SERVER_HELLO_TEMPLATE): TLS 1.3-вид, один
/// cipher, key_share-расширение с нулевым ключом. Длина 127 байт —
/// сервер_random на 11, session_id на 44, ключ на 89.
fn server_hello_template() -> Vec<u8> {
    let mut template = Vec::with_capacity(127);
    template.extend_from_slice(&[0x16, 0x03, 0x03, 0x00, 0x7a]); // record: handshake
    template.extend_from_slice(&[0x02, 0x00, 0x00, 0x76]); // ServerHello
    template.extend_from_slice(&[0x03, 0x03]); // legacy version
    template.extend_from_slice(&[0u8; 32]); // server_random
    template.push(0x20); // session_id length 32
    template.extend_from_slice(&[0u8; 32]); // session_id
    template.extend_from_slice(&[0x13, 0x01]); // TLS_AES_128_GCM_SHA256
    template.push(0x00); // compression
    template.extend_from_slice(&[0x00, 0x2e]); // extensions length
    template.extend_from_slice(&[0x00, 0x33, 0x00, 0x24, 0x00, 0x1d, 0x00, 0x20]); // key_share
    template.extend_from_slice(&[0u8; 32]); // public key
    template.extend_from_slice(&[0x00, 0x2b, 0x00, 0x02, 0x03, 0x04]); // versions: TLS 1.3
    debug_assert_eq!(template.len(), 127);
    template
}

pub struct VerifiedHello {
    pub client_random: [u8; 32],
    pub session_id: [u8; 32],
}

/// Проверка ClientHello (порт verify_client_hello): client_random =
/// HMAC(secret, hello_c_zeroed_random)[:28] ‖ (timestamp ^ hmac[28:32]).
pub fn verify_client_hello(data: &[u8], secret: &[u8; 16]) -> Option<VerifiedHello> {
    if data.len() < 43 || data[0] != TLS_RECORD_HANDSHAKE || data[5] != 0x01 {
        return None;
    }
    let client_random: [u8; 32] = data[CLIENT_RANDOM_OFFSET..CLIENT_RANDOM_OFFSET + 32]
        .try_into()
        .ok()?;

    let mut zeroed = data.to_vec();
    zeroed[CLIENT_RANDOM_OFFSET..CLIENT_RANDOM_OFFSET + 32].fill(0);

    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(&zeroed);
    let expected = mac.finalize().into_bytes();

    // Сравнение первых 28 байт без раннего выхода (constant-time).
    let mut diff = 0u8;
    for i in 0..28 {
        diff |= expected[i] ^ client_random[i];
    }
    if diff != 0 {
        return None;
    }

    let mut ts_bytes = [0u8; 4];
    for i in 0..4 {
        ts_bytes[i] = client_random[28 + i] ^ expected[28 + i];
    }
    let timestamp = u32::from_le_bytes(ts_bytes);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if (now - timestamp as i64).abs() > TIMESTAMP_TOLERANCE {
        return None;
    }

    let mut session_id = [0u8; 32];
    if data.len() >= SESSION_ID_OFFSET + 32 && data[43] == 0x20 {
        session_id.copy_from_slice(&data[SESSION_ID_OFFSET..SESSION_ID_OFFSET + 32]);
    }

    Some(VerifiedHello {
        client_random,
        session_id,
    })
}

/// Ответ клиенту (порт build_server_hello): шаблонный ServerHello с
/// эхом session_id, случайным «ключом», CCS и случайной appdata-записью;
/// server_random = HMAC(secret, client_random ‖ response).
pub fn build_server_hello(
    secret: &[u8; 16],
    client_random: &[u8; 32],
    session_id: &[u8; 32],
    rng: &mut impl RngCore,
) -> Vec<u8> {
    let mut sh = server_hello_template();
    sh[44..76].copy_from_slice(session_id);
    let mut pubkey = [0u8; 32];
    rng.fill_bytes(&mut pubkey);
    sh[89..121].copy_from_slice(&pubkey);

    let mut response = Vec::with_capacity(127 + 6 + 2100);
    response.extend_from_slice(&sh);
    response.extend_from_slice(&[0x14, 0x03, 0x03, 0x00, 0x01, 0x01]); // CCS
    let encrypted_size: u16 = rand::Rng::gen_range(rng, 1900..=2100);
    let mut encrypted = vec![0u8; encrypted_size as usize];
    rng.fill_bytes(&mut encrypted);
    response.extend_from_slice(&[0x17, 0x03, 0x03]);
    response.extend_from_slice(&encrypted_size.to_be_bytes());
    response.extend_from_slice(&encrypted);

    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(client_random);
    mac.update(&response);
    let server_random = mac.finalize().into_bytes();
    response[11..43].copy_from_slice(&server_random);
    response
}

/// Обёртка данных в appdata-записи TLS (порт wrap_tls_record).
pub fn wrap_tls_record(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5 * (data.len() / TLS_APPDATA_MAX + 1));
    for chunk in data.chunks(TLS_APPDATA_MAX) {
        out.extend_from_slice(&[TLS_RECORD_APPDATA, 0x03, 0x03]);
        out.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

/// Читающая половина FakeTLS-потока: извлекает полезную нагрузку из
/// appdata-записей, пропускает CCS, любой другой тип записи = конец
/// потока (как `_read_tls_payload` апстрима, возвращающий b'').
pub struct FakeTlsReader<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
}

impl<R: AsyncReadExt + Unpin> FakeTlsReader<R> {
    pub fn new(inner: R) -> Self {
        FakeTlsReader {
            inner,
            buf: Vec::new(),
            pos: 0,
        }
    }

    pub async fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.pos < self.buf.len() {
                let n = out.len().min(self.buf.len() - self.pos);
                out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                self.pos += n;
                if self.pos == self.buf.len() {
                    self.buf.clear();
                    self.pos = 0;
                }
                return Ok(n);
            }

            let mut header = [0u8; 5];
            match self.inner.read_exact(&mut header).await {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(error) => return Err(error),
            }
            let record_type = header[0];
            let record_len = u16::from_be_bytes([header[3], header[4]]) as usize;

            if record_type == TLS_RECORD_CCS {
                if record_len > 0 {
                    let mut skip = vec![0u8; record_len];
                    self.inner.read_exact(&mut skip).await?;
                }
                continue;
            }
            if record_type != TLS_RECORD_APPDATA {
                return Ok(0);
            }
            self.buf = vec![0u8; record_len];
            self.inner.read_exact(&mut self.buf).await?;
            self.pos = 0;
        }
    }

    pub async fn read_exact(&mut self, out: &mut [u8]) -> io::Result<()> {
        let mut filled = 0;
        while filled < out.len() {
            let n = self.read(&mut out[filled..]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "faketls stream closed",
                ));
            }
            filled += n;
        }
        Ok(())
    }
}

/// Пишущая половина FakeTLS-потока: данные уходят appdata-записями.
pub struct FakeTlsWriter<W> {
    inner: Mutex<W>,
}

impl<W: AsyncWriteExt + Unpin> FakeTlsWriter<W> {
    pub fn new(inner: W) -> Self {
        FakeTlsWriter {
            inner: Mutex::new(inner),
        }
    }

    pub async fn write_all(&self, data: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock().await;
        inner.write_all(&wrap_tls_record(data)).await
    }
}

/// Проброс «чужого» соединения на домен маскировки (порт
/// proxy_to_masking_domain): сканеры и клиенты без правильного секрета
/// получают настоящий ответ настоящего сайта.
pub async fn proxy_to_masking_domain(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    initial_data: &[u8],
    domain: &str,
    peer: &str,
) {
    let upstream = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect((domain, 443)),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            logger::warn(format!(
                "[{peer}] masking: cannot connect to {domain}:443: {error}"
            ));
            return;
        }
        Err(_) => {
            logger::warn(format!("[{peer}] masking: connect to {domain}:443 timed out"));
            return;
        }
    };
    logger::info(format!("[{peer}] masking -> {domain}:443"));
    let _ = upstream.set_nodelay(true);
    let (mut up_read, mut up_write) = upstream.into_split();
    let _ = up_write.write_all(initial_data).await;

    let client_to_up = async {
        let mut buf = [0u8; 16384];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if up_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = up_write.shutdown().await;
    };
    let up_to_client = async {
        let mut buf = [0u8; 16384];
        loop {
            match up_read.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = writer.shutdown().await;
    };
    tokio::join!(client_to_up, up_to_client);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 16] = [0x77; 16];

    /// Строит ClientHello так, как его строит клиент Telegram (mtg-схема).
    fn build_client_hello(
        secret: &[u8; 16],
        session_id: &[u8; 32],
        timestamp: u32,
    ) -> Vec<u8> {
        let mut hello = vec![0u8; 128];
        hello[0] = TLS_RECORD_HANDSHAKE;
        hello[1..3].copy_from_slice(&[0x03, 0x03]);
        let record_len = (hello.len() - 5) as u16;
        hello[3..5].copy_from_slice(&record_len.to_be_bytes());
        hello[5] = 0x01; // ClientHello
        hello[43] = 0x20;
        hello[44..76].copy_from_slice(session_id);

        // expected = HMAC(secret, hello с нулевым random)
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(&hello);
        let expected = mac.finalize().into_bytes();

        let ts_bytes = timestamp.to_le_bytes();
        for i in 0..28 {
            hello[CLIENT_RANDOM_OFFSET + i] = expected[i];
        }
        for i in 0..4 {
            hello[CLIENT_RANDOM_OFFSET + 28 + i] = ts_bytes[i] ^ expected[28 + i];
        }
        hello
    }

    fn now_unix() -> u32 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32
    }

    #[test]
    fn print_client_hello_hex() {
        let hello = build_client_hello(&SECRET, &[0x33; 32], now_unix());
        let hex: String = hello.iter().map(|b| format!("{b:02x}")).collect();
        println!("HELLO_HEX={hex}");
    }

    #[test]
    fn verifies_valid_client_hello() {
        let session_id = [0x42u8; 32];
        let hello = build_client_hello(&SECRET, &session_id, now_unix());
        let verified = verify_client_hello(&hello, &SECRET).expect("must verify");
        assert_eq!(verified.session_id, session_id);
        // client_random возвращается как есть.
        let expected_random: [u8; 32] = hello[11..43].try_into().unwrap();
        assert_eq!(verified.client_random, expected_random);
    }

    #[test]
    fn rejects_wrong_secret_and_stale_timestamp() {
        let hello = build_client_hello(&SECRET, &[0u8; 32], now_unix());
        assert!(verify_client_hello(&hello, &[0x01; 16]).is_none());

        let stale = build_client_hello(&SECRET, &[0u8; 32], now_unix() - 300);
        assert!(verify_client_hello(&stale, &SECRET).is_none());
    }

    #[test]
    fn rejects_non_handshake() {
        let mut hello = build_client_hello(&SECRET, &[0u8; 32], now_unix());
        hello[0] = 0x17;
        assert!(verify_client_hello(&hello, &SECRET).is_none());
        assert!(verify_client_hello(&hello[..20], &SECRET).is_none());
    }

    #[test]
    fn server_hello_is_self_consistent() {
        let client_random = [0x11u8; 32];
        let session_id = [0x22u8; 32];
        let response = build_server_hello(&SECRET, &client_random, &session_id, &mut rand::rngs::OsRng);

        // Структура: ServerHello-запись (127B) + CCS + appdata.
        assert_eq!(response[0], 0x16);
        assert_eq!(&response[127..133], &[0x14, 0x03, 0x03, 0x00, 0x01, 0x01]);
        assert_eq!(response[133], 0x17);

        // Эхо session_id.
        assert_eq!(&response[44..76], &session_id);

        // server_random = HMAC(secret, client_random || response_c_random_нул).
        let mut without_random = response.clone();
        without_random[11..43].fill(0);
        let mut mac = HmacSha256::new_from_slice(&SECRET).unwrap();
        mac.update(&client_random);
        mac.update(&without_random);
        let expected = mac.finalize().into_bytes();
        assert_eq!(&response[11..43], &expected[..32]);
    }

    #[test]
    fn wraps_into_16k_records() {
        let data = vec![0xabu8; TLS_APPDATA_MAX * 2 + 100];
        let wrapped = wrap_tls_record(&data);
        // Три записи: 16384 + 16384 + 100 + заголовки.
        assert_eq!(
            wrapped.len(),
            (16384 + 5) + (16384 + 5) + (100 + 5)
        );
        assert_eq!(wrapped[0], 0x17);
        assert_eq!(&wrapped[1..3], &[0x03, 0x03]);
        assert_eq!(u16::from_be_bytes([wrapped[3], wrapped[4]]), 16384);
    }

    #[tokio::test]
    async fn faketls_reader_writer_roundtrip_through_duplex() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        // Пишущая сторона: обычные байты превращаются в appdata-записи.
        let writer = FakeTlsWriter::new(client_write);
        writer.write_all(b"ping").await.unwrap();
        let mut framed = [0u8; 9];
        server_read.read_exact(&mut framed).await.unwrap();
        assert_eq!(&framed[..5], &[0x17, 0x03, 0x03, 0x00, 0x04]);
        assert_eq!(&framed[5..], b"ping");

        // Читающая сторона: appdata извлекаются, CCS пропускается,
        // чужой тип записи завершает поток.
        let mut reader = FakeTlsReader::new(client_read);
        let wire = [
            &[0x17, 0x03, 0x03, 0x00, 0x05][..],
            b"hello",
            &[0x14, 0x03, 0x03, 0x00, 0x01, 0x01][..], // CCS
            &[0x17, 0x03, 0x03, 0x00, 0x05][..],
            b"world",
        ]
        .concat();
        server_write.write_all(&wire).await.unwrap();

        let mut buffer = [0u8; 10];
        reader.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"helloworld");

        // Запись не-appdata (alert) читается как конец потока.
        server_write
            .write_all(&[0x15, 0x03, 0x03, 0x00, 0x02, 0x01, 0x00])
            .await
            .unwrap();
        let mut tail = [0u8; 4];
        assert_eq!(reader.read(&mut tail).await.unwrap(), 0);
    }
}
