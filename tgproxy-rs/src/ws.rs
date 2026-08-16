//! Минимальный RFC6455-клиент поверх tokio + rustls (порт RawWebSocket).
//!
//! Отличие от апстрима: TLS к DC верифицируется по webpki-корням с
//! проверкой SNI — вместо `CERT_NONE`.

use std::sync::Arc;

use base64::Engine;
use rand::RngCore;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

pub const MAX_MESSAGE_LEN: usize = 16 * 1024 * 1024;
const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

const OP_CONT: u8 = 0x0;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

#[derive(Debug)]
pub enum ConnectError {
    Timeout,
    Io(std::io::Error),
    Tls(std::io::Error),
    Handshake(WsHandshakeError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Timeout => write!(f, "timed out"),
            ConnectError::Io(error) | ConnectError::Tls(error) => write!(f, "{error}"),
            ConnectError::Handshake(error) => write!(f, "{error}"),
        }
    }
}

#[derive(Debug)]
pub struct WsHandshakeError {
    pub status_code: u16,
    pub status_line: String,
    pub location: Option<String>,
}

impl std::fmt::Display for WsHandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status_code, self.status_line)?;
        if let Some(location) = &self.location {
            write!(f, " (location: {location})")?;
        }
        Ok(())
    }
}

pub fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Пишущая половина: отправка фреймов к DC. Разделена через Mutex —
/// насосы моста и ответы на ping идут из разных задач.
pub struct WsWriter {
    write: Mutex<WriteHalf<TlsStream<TcpStream>>>,
}

impl WsWriter {
    /// Отправка бинарного сообщения одним фреймом.
    pub async fn send(&self, data: &[u8]) -> std::io::Result<()> {
        let mut write = self.write.lock().await;
        send_frame(&mut *write, OP_BINARY, data).await
    }

    /// Пакеты одного чанка уходят очередью фреймов с одним flush.
    pub async fn send_batch(&self, parts: &[Vec<u8>]) -> std::io::Result<()> {
        let mut write = self.write.lock().await;
        for part in parts {
            send_frame(&mut *write, OP_BINARY, part).await?;
        }
        write.flush().await
    }

    pub async fn close(&self) {
        let mut write = self.write.lock().await;
        let _ = send_frame(&mut *write, OP_CLOSE, &[]).await;
    }

    async fn send_pong(&self, payload: &[u8]) {
        let mut write = self.write.lock().await;
        let _ = send_frame(&mut *write, OP_PONG, payload).await;
    }
}

pub struct WsReader {
    read: BufReader<ReadHalf<TlsStream<TcpStream>>>,
    writer: Arc<WsWriter>,
}

impl WsReader {
    /// Следующее бинарное сообщение (с реассемблированием фрагментов и
    /// ответом на пинги). `None` — соединение закрыто любой из сторон.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let mut fragment = Vec::new();
        let mut fragmenting = false;
        loop {
            let frame = read_frame(&mut self.read).await?;
            match frame.opcode {
                OP_BINARY => {
                    if fragmenting {
                        return None; // новый дата-фрейм посреди фрагментации
                    }
                    if frame.fin {
                        return Some(frame.payload);
                    }
                    fragmenting = true;
                    fragment = frame.payload;
                }
                OP_CONT => {
                    if !fragmenting {
                        return None; // continuation без начального data frame
                    }
                    fragment.extend_from_slice(&frame.payload);
                    if fragment.len() > MAX_MESSAGE_LEN {
                        return None;
                    }
                    if frame.fin {
                        return Some(std::mem::take(&mut fragment));
                    }
                }
                OP_PING => {
                    let pong = frame.payload[..frame.payload.len().min(125)].to_vec();
                    self.writer.send_pong(&pong).await;
                }
                OP_PONG => {}
                OP_CLOSE => {
                    let code = if frame.payload.len() >= 2 {
                        u16::from_be_bytes([frame.payload[0], frame.payload[1]])
                    } else {
                        1005
                    };
                    crate::logger::info(format!("ws close received, code {code}"));
                    self.writer.close().await;
                    return None;
                }
                _ => return None,
            }
        }
    }
}

/// TCP к `ip:443`, TLS с SNI/верификацией `domain`, HTTP Upgrade на
/// `path` с сабпротоколом `binary`. Возвращает обе половины соединения.
pub async fn connect(
    ip: &str,
    domain: &str,
    path: &str,
    timeout: std::time::Duration,
    connector: &TlsConnector,
) -> Result<(WsReader, Arc<WsWriter>), ConnectError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let tcp = tokio::time::timeout_at(deadline, TcpStream::connect((ip, 443)))
        .await
        .map_err(|_| ConnectError::Timeout)?
        .map_err(ConnectError::Io)?;
    let _ = tcp.set_nodelay(true);

    let server_name =
        rustls::pki_types::ServerName::try_from(domain.to_string()).map_err(|error| {
            ConnectError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid server name: {error}"),
            ))
        })?;
    let tls = tokio::time::timeout_at(deadline, connector.connect(server_name, tcp))
        .await
        .map_err(|_| ConnectError::Timeout)?
        .map_err(ConnectError::Tls)?;

    let (read_half, write_half) = tokio::io::split(tls);
    let mut read = BufReader::new(read_half);
    let mut write = write_half;

    let mut key_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    let ws_key = base64::engine::general_purpose::STANDARD.encode(key_bytes);
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {domain}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {ws_key}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Protocol: binary\r\n\
         \r\n"
    );

    let headers_raw = tokio::time::timeout_at(deadline, async {
        write.write_all(request.as_bytes()).await?;
        write.flush().await?;

        // Заголовки ответа до пустой строки, с разумным лимитом.
        let mut headers_raw = Vec::with_capacity(1024);
        loop {
            let mut byte = [0u8; 1];
            let n = read.read(&mut byte).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof during ws handshake",
                ));
            }
            headers_raw.push(byte[0]);
            if headers_raw.len() > 8192 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ws handshake response too large",
                ));
            }
            if headers_raw.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        Ok(headers_raw)
    })
    .await
    .map_err(|_| ConnectError::Timeout)?
    .map_err(ConnectError::Io)?;

    validate_server_handshake(&headers_raw, &ws_key)?;

    let writer = Arc::new(WsWriter {
        write: Mutex::new(write),
    });
    Ok((
        WsReader {
            read,
            writer: writer.clone(),
        },
        writer,
    ))
}

fn validate_server_handshake(headers_raw: &[u8], ws_key: &str) -> Result<(), ConnectError> {
    let text = std::str::from_utf8(headers_raw).map_err(|error| {
        ConnectError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("non-UTF8 websocket response: {error}"),
        ))
    })?;
    let mut lines = text.split("\r\n");
    let first_line = lines.next().unwrap_or_default();
    let status_code: u16 = first_line
        .split_ascii_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let mut location = None;
    let mut upgrade = None;
    let mut connection = None;
    let mut accept = None;
    let mut protocol = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(invalid_handshake("malformed HTTP header"));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("location") {
            location = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("upgrade") {
            upgrade = Some(value);
        } else if name.eq_ignore_ascii_case("connection") {
            connection = Some(value);
        } else if name.eq_ignore_ascii_case("sec-websocket-accept") {
            accept = Some(value);
        } else if name.eq_ignore_ascii_case("sec-websocket-protocol") {
            protocol = Some(value);
        }
    }
    if status_code != 101 {
        return Err(ConnectError::Handshake(WsHandshakeError {
            status_code,
            status_line: first_line.to_string(),
            location,
        }));
    }
    if !first_line.starts_with("HTTP/1.1 ") {
        return Err(invalid_handshake("websocket response is not HTTP/1.1"));
    }
    let has_token = |value: Option<&str>, expected: &str| {
        value.is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case(expected))
        })
    };
    if !has_token(upgrade, "websocket") {
        return Err(invalid_handshake("missing Upgrade: websocket"));
    }
    if !has_token(connection, "upgrade") {
        return Err(invalid_handshake("missing Connection: Upgrade"));
    }
    let expected_accept = websocket_accept(ws_key);
    if accept != Some(expected_accept.as_str()) {
        return Err(invalid_handshake("invalid Sec-WebSocket-Accept"));
    }
    if !protocol.is_some_and(|value| value.eq_ignore_ascii_case("binary")) {
        return Err(invalid_handshake(
            "server did not select binary subprotocol",
        ));
    }
    Ok(())
}

fn invalid_handshake(message: &str) -> ConnectError {
    ConnectError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.to_string(),
    ))
}

fn websocket_accept(ws_key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(ws_key.as_bytes());
    sha1.update(WS_GUID);
    base64::engine::general_purpose::STANDARD.encode(sha1.finalize())
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

async fn send_frame<W>(write: &mut W, opcode: u8, payload: &[u8]) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut frame = Vec::with_capacity(payload.len() + 14);
    encode_frame(&mut frame, opcode, payload);
    write.write_all(&frame).await?;
    write.flush().await
}

/// Кодирование клиентского фрейма: FIN=1, маска обязательна для
/// клиентских фреймов по RFC 6455.
pub fn encode_frame(out: &mut Vec<u8>, opcode: u8, payload: &[u8]) {
    let mut mask = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut mask);

    out.push(0x80 | opcode);
    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    let start = out.len();
    out.extend_from_slice(payload);
    for (i, byte) in out[start..].iter_mut().enumerate() {
        *byte ^= mask[i % 4];
    }
}

async fn read_frame<R>(read: &mut R) -> Option<Frame>
where
    R: AsyncReadExt + Unpin,
{
    let mut header = [0u8; 2];
    read.read_exact(&mut header).await.ok()?;
    let fin = header[0] & 0x80 != 0;
    if header[0] & 0x70 != 0 {
        return None; // RSV-биты: расширения не согласованы
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    if masked {
        return None; // сервер не имеет права маскировать свои фреймы
    }
    let len7 = (header[1] & 0x7f) as usize;

    let payload_len = match len7 {
        126 => {
            let mut extended = [0u8; 2];
            read.read_exact(&mut extended).await.ok()?;
            u16::from_be_bytes(extended) as usize
        }
        127 => {
            let mut extended = [0u8; 8];
            read.read_exact(&mut extended).await.ok()?;
            if extended[0] & 0x80 != 0 {
                return None;
            }
            let len = usize::try_from(u64::from_be_bytes(extended)).ok()?;
            if len > MAX_MESSAGE_LEN {
                return None;
            }
            len
        }
        other => other,
    };
    // Контрольные фреймы не бывают больше 125 байт и не фрагментируются.
    if opcode >= 0x8 && (payload_len > 125 || !fin) {
        return None;
    }

    if payload_len > MAX_MESSAGE_LEN {
        return None;
    }
    let mut payload = vec![0u8; payload_len];
    read.read_exact(&mut payload).await.ok()?;
    Some(Frame {
        fin,
        opcode,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encrypted_req_pq(relay: &[u8; crate::obf2::HANDSHAKE_LEN]) -> Vec<u8> {
        use rand::RngCore as _;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch");
        let fraction = ((u64::from(now.subsec_nanos())) << 32) / 1_000_000_000;
        let message_id = ((now.as_secs() << 32) | fraction) & !3;

        let mut packet = Vec::with_capacity(41);
        packet.push(0x0a); // abridged length: 40 bytes / 4
        packet.extend_from_slice(&0u64.to_le_bytes()); // auth_key_id: unencrypted
        packet.extend_from_slice(&message_id.to_le_bytes());
        packet.extend_from_slice(&20u32.to_le_bytes());
        packet.extend_from_slice(&0xbe7e8ef1u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        packet.extend_from_slice(&nonce);

        let key: [u8; 32] = relay[8..40].try_into().unwrap();
        let iv: [u8; 16] = relay[40..56].try_into().unwrap();
        let mut cipher = crate::crypto::CtrCipher::new(&key, &iv);
        cipher.skip_64();
        cipher.apply(&mut packet);
        packet
    }

    /// Синхронное декодирование полного набора фреймов из буфера.
    fn decode_frames(buffer: &[u8]) -> Vec<Frame> {
        let mut offset = 0;
        let mut frames = Vec::new();
        while offset < buffer.len() {
            let fin = buffer[offset] & 0x80 != 0;
            let opcode = buffer[offset] & 0x0f;
            let masked = buffer[offset + 1] & 0x80 != 0;
            let len7 = (buffer[offset + 1] & 0x7f) as usize;
            let mut cursor = offset + 2;
            let payload_len = match len7 {
                126 => {
                    let len =
                        u16::from_be_bytes(buffer[cursor..cursor + 2].try_into().unwrap()) as usize;
                    cursor += 2;
                    len
                }
                127 => {
                    let len =
                        u64::from_be_bytes(buffer[cursor..cursor + 8].try_into().unwrap()) as usize;
                    cursor += 8;
                    len
                }
                other => other,
            };
            let mut mask: Option<[u8; 4]> = None;
            if masked {
                mask = Some(buffer[cursor..cursor + 4].try_into().unwrap());
                cursor += 4;
            }
            let mut payload = buffer[cursor..cursor + payload_len].to_vec();
            if let Some(mask) = mask {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
            cursor += payload_len;
            frames.push(Frame {
                fin,
                opcode,
                payload,
            });
            offset = cursor;
        }
        frames
    }

    #[test]
    fn encode_decode_roundtrip_small_payload() {
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, b"hello");
        let frames = decode_frames(&buffer);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].fin);
        assert_eq!(frames[0].opcode, OP_BINARY);
        assert_eq!(frames[0].payload, b"hello");
    }

    #[test]
    fn encode_decode_extended_lengths() {
        for len in [125usize, 126, 65535, 65536, 70000] {
            let payload = vec![0xa5; len];
            let mut buffer = Vec::new();
            encode_frame(&mut buffer, OP_BINARY, &payload);
            let frames = decode_frames(&buffer);
            assert_eq!(frames.len(), 1, "len {len}");
            assert_eq!(frames[0].payload, payload, "len {len}");
        }
    }

    #[test]
    fn multiple_frames_in_one_buffer() {
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, b"one");
        encode_frame(&mut buffer, OP_PING, b"");
        encode_frame(&mut buffer, OP_BINARY, b"two");
        let frames = decode_frames(&buffer);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].payload, b"one");
        assert_eq!(frames[1].opcode, OP_PING);
        assert_eq!(frames[2].payload, b"two");
    }

    #[test]
    fn relay_init_sized_frame_roundtrip() {
        // 64-байтный relay init — первый фрейм сессии.
        let payload = vec![0x33u8; 64];
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, &payload);
        let frames = decode_frames(&buffer);
        assert_eq!(frames[0].payload, payload);
    }

    #[test]
    fn validates_rfc6455_upgrade_headers() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let valid = b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: keep-alive, Upgrade\r\n\
Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
Sec-WebSocket-Protocol: binary\r\n\r\n";
        assert!(validate_server_handshake(valid, key).is_ok());

        let invalid = b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: wrong\r\n\
Sec-WebSocket-Protocol: binary\r\n\r\n";
        let error = validate_server_handshake(invalid, key).unwrap_err();
        assert!(error.to_string().contains("Sec-WebSocket-Accept"));
    }

    #[tokio::test]
    async fn rejects_masked_server_frames() {
        let (mut write, mut read) = tokio::io::duplex(128);
        let mut masked = Vec::new();
        encode_frame(&mut masked, OP_BINARY, b"masked");
        write.write_all(&masked).await.unwrap();
        drop(write);
        assert!(read_frame(&mut read).await.is_none());
    }

    /// Живая проверка против настоящего DC2 Telegram: WS-хендшейк с
    /// верифицированным TLS, отправка relay init + настоящего req_pq_multi,
    /// ожидание непустого ответа DC. Требует сеть — запускать явно:
    /// `cargo test live_dc -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires network access to Telegram DC"]
    async fn live_dc2_relay_handshake() {
        let connector = tls_connector();
        let (mut reader, writer) = connect(
            "149.154.167.220",
            "kws2.web.telegram.org",
            "/apiws",
            std::time::Duration::from_secs(10),
            &connector,
        )
        .await
        .expect("ws connect to DC2 with verified TLS");

        let relay = crate::obf2::make_relay_init(
            crate::obf2::ProtoTag::Abridged,
            2,
            &mut rand::rngs::OsRng,
        );
        writer.send(&relay).await.expect("send relay init");
        writer
            .send(&encrypted_req_pq(&relay))
            .await
            .expect("send req_pq_multi");

        let response = tokio::time::timeout(std::time::Duration::from_secs(10), reader.recv())
            .await
            .expect("timeout waiting for resPQ")
            .expect("DC closed connection without resPQ");
        assert!(!response.is_empty(), "DC returned an empty response");
        writer.close().await;
    }
}
